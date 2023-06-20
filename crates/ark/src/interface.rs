//
// r_interface.rs
//
// Copyright (C) 2022 Posit Software, PBC. All rights reserved.
//
//

use std::ffi::*;
use std::os::raw::c_uchar;
use std::os::raw::c_void;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Once;
use std::time::Duration;
use std::time::SystemTime;

use amalthea::events::BusyEvent;
use amalthea::events::PositronEvent;
use amalthea::events::ShowMessageEvent;
use amalthea::socket::iopub::IOPubMessage;
use amalthea::wire::stream::Stream;
use bus::Bus;
use crossbeam::channel::Receiver;
use crossbeam::channel::RecvTimeoutError;
use crossbeam::channel::Sender;
use harp::interrupts::RInterruptsSuspendedScope;
use harp::lock::R_RUNTIME_LOCK;
use harp::lock::R_RUNTIME_LOCK_COUNT;
use harp::routines::r_register_routines;
use harp::utils::r_get_option;
use libR_sys::*;
use log::*;
use nix::sys::signal::*;
use parking_lot::ReentrantMutexGuard;
use stdext::*;

use crate::errors;
use crate::help_proxy;
use crate::kernel::Kernel;
use crate::kernel::KernelInfo;
use crate::lsp::events::EVENTS;
use crate::modules;
use crate::plots::graphics_device;
use crate::request::Request;

extern "C" {
    pub static mut R_running_as_main_program: ::std::os::raw::c_int;
    pub static mut R_SignalHandlers: ::std::os::raw::c_int;
    pub static mut R_Interactive: Rboolean;
    pub static mut R_Consolefile: *mut FILE;
    pub static mut R_Outputfile: *mut FILE;

    pub static mut ptr_R_WriteConsole: ::std::option::Option<
        unsafe extern "C" fn(arg1: *const ::std::os::raw::c_char, arg2: ::std::os::raw::c_int),
    >;

    pub static mut ptr_R_WriteConsoleEx: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *const ::std::os::raw::c_char,
            arg2: ::std::os::raw::c_int,
            arg3: ::std::os::raw::c_int,
        ),
    >;

    pub static mut ptr_R_ReadConsole: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *const ::std::os::raw::c_char,
            arg2: *mut ::std::os::raw::c_uchar,
            arg3: ::std::os::raw::c_int,
            arg4: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int,
    >;

    pub static mut ptr_R_ShowMessage:
        ::std::option::Option<unsafe extern "C" fn(arg1: *const ::std::os::raw::c_char)>;

    pub static mut ptr_R_Busy:
        ::std::option::Option<unsafe extern "C" fn(arg1: ::std::os::raw::c_int)>;

    pub fn R_HomeDir() -> *mut ::std::os::raw::c_char;

    // NOTE: Some of these routines don't really return (or use) void pointers,
    // but because we never introspect these values directly and they're always
    // passed around in R as pointers, it suffices to just use void pointers.
    fn R_checkActivity(usec: i32, ignore_stdin: i32) -> *const c_void;
    fn R_runHandlers(handlers: *const c_void, fdset: *const c_void);
    fn R_ProcessEvents();
    fn run_Rmainloop();

    pub static mut R_wait_usec: i32;
    pub static mut R_InputHandlers: *const c_void;
    pub static mut R_PolledEvents: Option<unsafe extern "C" fn()>;
}

fn initialize_signal_handlers() {
    // Reset the signal block.
    //
    // This appears to be necessary on macOS; 'sigprocmask()' specifically
    // blocks the signals in _all_ threads associated with the process, even
    // when called from a spawned child thread. See:
    //
    // https://github.com/opensource-apple/xnu/blob/0a798f6738bc1db01281fc08ae024145e84df927/bsd/kern/kern_sig.c#L1238-L1285
    // https://github.com/opensource-apple/xnu/blob/0a798f6738bc1db01281fc08ae024145e84df927/bsd/kern/kern_sig.c#L796-L839
    //
    // and note that 'sigprocmask()' uses 'block_procsigmask()' to apply the
    // requested block to all threads in the process:
    //
    // https://github.com/opensource-apple/xnu/blob/0a798f6738bc1db01281fc08ae024145e84df927/bsd/kern/kern_sig.c#L571-L599
    //
    // We may need to re-visit this on Linux later on, since 'sigprocmask()' and
    // 'pthread_sigmask()' may only target the executing thread there.
    //
    // The behavior of 'sigprocmask()' is unspecified after all, so we're really
    // just relying on what the implementation happens to do.
    let mut sigset = SigSet::empty();
    sigset.add(SIGINT);
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&sigset), None).unwrap();

    // Unblock signals on this thread.
    pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&sigset), None).unwrap();

    // Install an interrupt handler.
    unsafe {
        signal(SIGINT, SigHandler::Handler(handle_interrupt)).unwrap();
    }
}

extern "C" fn handle_interrupt(_signal: libc::c_int) {
    unsafe {
        R_interrupts_pending = 1;
    }
}

// --- Globals ---
// These values must be global in order for them to be accessible from R
// callbacks, which do not have a facility for passing or returning context.

/// The global R kernel state.
pub static mut KERNEL: Option<Arc<Mutex<Kernel>>> = None;

/// A lock guard, used to manage access to the R runtime.  The main thread holds
/// the lock by default, but releases it at opportune times to allow the LSP to
/// access the R runtime where appropriate.
pub static mut R_RUNTIME_LOCK_GUARD: Option<ReentrantMutexGuard<()>> = None;

/// A channel that sends prompts from R to the kernel
static mut RPROMPT_SEND: Option<Mutex<Sender<PromptInfo>>> = None;

/// A channel that receives console input from the kernel and sends it to R;
/// sending empty input (None) tells R to shut down
static mut CONSOLE_RECV: Option<Mutex<Receiver<Option<String>>>> = None;

/// Ensures that the kernel is only ever initialized once
static INIT: Once = Once::new();

pub unsafe fn process_events() {
    // Don't process interrupts in this scope.
    let _interrupts_suspended = RInterruptsSuspendedScope::new();

    // Process regular R events.
    R_ProcessEvents();

    // Run handlers if we have data available. This is necessary
    // for things like the HTML help server, which will listen
    // for requests on an open socket() which would then normally
    // be handled in a select() call when reading input from stdin.
    //
    // https://github.com/wch/r-source/blob/4ca6439c1ffc76958592455c44d83f95d5854b2a/src/unix/sys-std.c#L1084-L1086
    //
    // We run this in a loop just to make sure the R help server can
    // be as responsive as possible when rendering help pages.
    let mut fdset = R_checkActivity(0, 1);
    while fdset != std::ptr::null_mut() {
        R_runHandlers(R_InputHandlers, fdset);
        fdset = R_checkActivity(0, 1);
    }

    // Run pending finalizers. We need to do this eagerly as otherwise finalizers
    // might end up being executed on the LSP thread.
    // https://github.com/rstudio/positron/issues/431
    R_RunPendingFinalizers();

    // Render pending plots.
    graphics_device::on_process_events();
}

fn on_console_input(buf: *mut c_uchar, buflen: c_int, mut input: String) {
    // TODO: What if the input is too large for the buffer?
    input.push_str("\n");
    if input.len() > buflen as usize {
        info!("Error: input too large for buffer.");
        return;
    }

    let src = CString::new(input).unwrap();
    unsafe {
        libc::strcpy(buf as *mut c_char, src.as_ptr());
    }
}

/// Invoked by R to read console input from the user.
///
/// * `prompt` - The prompt shown to the user
/// * `buf`    - Pointer to buffer to receive the user's input (type `CONSOLE_BUFFER_CHAR`)
/// * `buflen` - Size of the buffer to receiver user's input
/// * `hist`   - Whether to add the input to the history (1) or not (0)
///
#[no_mangle]
pub extern "C" fn r_read_console(
    prompt: *const c_char,
    buf: *mut c_uchar,
    buflen: c_int,
    _hist: c_int,
) -> i32 {
    let info = prompt_info(prompt);
    debug!("R prompt: {}", info.prompt);

    // TODO: Can we remove this below code?
    // If the prompt begins with "Save workspace", respond with (n)
    //
    // NOTE: Should be able to overwrite the `Cleanup` frontend method.
    // This would also help with detecting normal exits versus crashes.
    if info.prompt.starts_with("Save workspace") {
        let n = CString::new("n\n").unwrap();
        unsafe {
            libc::strcpy(buf as *mut c_char, n.as_ptr());
        }
        return 1;
    }

    // TODO: if R prompt is +, we need to tell the user their input is incomplete
    let mutex = unsafe { RPROMPT_SEND.as_ref().unwrap() };
    let r_prompt_tx = mutex.lock().unwrap();
    r_prompt_tx.send(info).unwrap();

    let mutex = unsafe { CONSOLE_RECV.as_ref().unwrap() };
    let receiver = mutex.lock().unwrap();

    // Match with a timeout. Necessary because we need to
    // pump the event loop while waiting for console input.
    //
    // Alternatively, we could try to figure out the file
    // descriptors that R has open and select() on those for
    // available data?
    loop {
        // Release the R runtime lock while we're waiting for input.
        unsafe { R_RUNTIME_LOCK_GUARD = None };

        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(response) => {
                // Take back the lock after we've received some console input.
                unsafe { R_RUNTIME_LOCK_GUARD = Some(R_RUNTIME_LOCK.lock()) };

                // If we received an interrupt while the user was typing input,
                // we can assume the interrupt was 'handled' and so reset the flag.
                unsafe {
                    R_interrupts_pending = 0;
                }

                // Process events.
                unsafe { process_events() };

                if let Some(input) = response {
                    on_console_input(buf, buflen, input);
                }

                return 1;
            },

            Err(error) => {
                unsafe { R_RUNTIME_LOCK_GUARD = Some(R_RUNTIME_LOCK.lock()) };

                use RecvTimeoutError::*;
                match error {
                    Timeout => {
                        // Process events.
                        unsafe { process_events() };

                        // Keep waiting for console input.
                        continue;
                    },

                    Disconnected => {
                        return 1;
                    },
                }
            },
        }
    }
}

/**
 * This struct represents the data that we wish R would pass to
 * `ReadConsole()` methods. We need this information to determine what kind
 * of prompt we are dealing with.
 *
 * TODO: `browser` field */
pub struct PromptInfo {
    /** The prompt string to be presented to the user */
    prompt: String,

    /** Whether the last input didn't fully parse and R is waiting for more
     * input */
    incomplete: bool,

    /** Whether this is a prompt from a fresh REPL iteration (browser or
     * top level) or a prompt from some user code, e.g. via `readline()` */
    user_request: bool,
}

fn prompt_info(prompt_c: *const c_char) -> PromptInfo {
    let prompt_slice = unsafe { CStr::from_ptr(prompt_c) };
    let prompt = prompt_slice.to_string_lossy().into_owned();

    // The request is incomplete if we see the continue prompt
    let continue_prompt = unsafe { r_get_option::<String>("continue").unwrap() };
    let incomplete = prompt == continue_prompt;

    // If the current prompt doesn't match the default prompt, assume that
    // we're reading use input, e.g. via 'readline()'.
    let default_prompt = unsafe { r_get_option::<String>("prompt").unwrap() };
    let user_request = !incomplete && prompt != default_prompt;

    if incomplete {
        trace!("Got R prompt '{}', marking request incomplete", prompt);
    } else if user_request {
        trace!("Got R prompt '{}', asking user for input", prompt);
    }

    return PromptInfo {
        prompt,
        incomplete,
        user_request,
    };
}

/**
 * Invoked by R to write output to the console.
 */
#[no_mangle]
pub extern "C" fn r_write_console(buf: *const c_char, _buflen: i32, otype: i32) {
    let content = unsafe { CStr::from_ptr(buf) };
    let mutex = unsafe { KERNEL.as_ref().unwrap() };
    let stream = if otype == 0 {
        Stream::Stdout
    } else {
        Stream::Stderr
    };
    let mut kernel = mutex.lock().unwrap();
    kernel.write_console(content.to_str().unwrap(), stream);
}

/**
 * Invoked by R to show a message to the user.
 */
#[no_mangle]
pub extern "C" fn r_show_message(buf: *const c_char) {
    // Convert the message to a string
    let message = unsafe { CStr::from_ptr(buf) };

    // Wait for a lock on the kernel
    let mutex = unsafe { KERNEL.as_ref().unwrap() };
    let kernel = mutex.lock().unwrap();

    // Create an event representing the message
    let event = PositronEvent::ShowMessage(ShowMessageEvent {
        message: message.to_str().unwrap().to_string(),
    });

    // Have the kernel deliver the event to the front end
    kernel.send_event(event);
}

/**
 * Invoked by R to change busy state
 */
#[no_mangle]
pub extern "C" fn r_busy(which: i32) {
    // Ensure signal handlers are initialized.
    //
    // We perform this awkward dance because R tries to set and reset
    // the interrupt signal handler here, using 'signal()':
    //
    // https://github.com/wch/r-source/blob/e7a21904029917a63b4717b53a173b01eeabcc7b/src/unix/sys-std.c#L171-L178
    //
    // However, it seems like this can cause the old interrupt handler to be
    // 'moved' to a separate thread, such that interrupts end up being handled
    // on a thread different from the R execution thread. At least, on macOS.
    initialize_signal_handlers();

    // Wait for a lock on the kernel
    let mutex = unsafe { KERNEL.as_ref().unwrap() };
    let kernel = mutex.lock().unwrap();

    // Create an event representing the new busy state
    let event = PositronEvent::Busy(BusyEvent { busy: which != 0 });

    // Have the kernel deliver the event to the front end
    kernel.send_event(event);
}

pub unsafe extern "C" fn r_polled_events() {
    // Check for pending tasks.
    let count = R_RUNTIME_LOCK_COUNT.load(std::sync::atomic::Ordering::Acquire);
    if count == 0 {
        return;
    }

    info!(
        "{} thread(s) are waiting; the main thread is releasing the R runtime lock.",
        count
    );
    let now = SystemTime::now();

    // `bump()` does a fair unlock, giving other threads
    // waiting for the lock a chance to acquire it, and then
    // relocks it.
    ReentrantMutexGuard::bump(R_RUNTIME_LOCK_GUARD.as_mut().unwrap());

    info!(
        "The main thread re-acquired the R runtime lock after {} milliseconds.",
        now.elapsed().unwrap().as_millis()
    );
}

pub fn start_r(
    iopub_tx: Sender<IOPubMessage>,
    kernel_init_tx: Bus<KernelInfo>,
    shell_request_rx: Receiver<Request>,
) {
    use std::borrow::BorrowMut;

    // The main thread owns the R runtime lock by default, but releases
    // it when appropriate to give other threads a chance to execute.
    unsafe { R_RUNTIME_LOCK_GUARD = Some(R_RUNTIME_LOCK.lock()) };

    // Start building the channels + kernel objects
    let (console_tx, console_rx) = crossbeam::channel::unbounded();
    let (rprompt_tx, rprompt_rx) = crossbeam::channel::unbounded();
    let kernel = Kernel::new(iopub_tx, console_tx.clone(), kernel_init_tx);

    // Initialize kernel (ensure we only do this once!)
    INIT.call_once(|| unsafe {
        *CONSOLE_RECV.borrow_mut() = Some(Mutex::new(console_rx));
        *RPROMPT_SEND.borrow_mut() = Some(Mutex::new(rprompt_tx));
        *KERNEL.borrow_mut() = Some(Arc::new(Mutex::new(kernel)));
    });

    // Start thread to listen to execution requests
    spawn!("ark-execution", move || {
        listen(shell_request_rx, rprompt_rx)
    });

    unsafe {
        let mut args = cargs!["ark", "--interactive"];
        R_running_as_main_program = 1;
        R_SignalHandlers = 0;
        Rf_initialize_R(args.len() as i32, args.as_mut_ptr() as *mut *mut c_char);

        // Initialize the interrupt handler.
        initialize_signal_handlers();

        // Disable stack checking; R doesn't know the starting point of the
        // stack for threads other than the main thread. Consequently, it will
        // report a stack overflow if we don't disable it. This is a problem
        // on all platforms, but is most obvious on aarch64 Linux due to how
        // thread stacks are allocated on that platform.
        //
        // See https://cran.r-project.org/doc/manuals/R-exts.html#Threading-issues
        // for more information.
        R_CStackLimit = usize::MAX;

        // Log the value of R_HOME, so we can know if something hairy is afoot
        let home = CStr::from_ptr(R_HomeDir());
        trace!("R_HOME: {:?}", home);

        // Mark R session as interactive
        R_Interactive = 1;

        // Redirect console
        R_Consolefile = std::ptr::null_mut();
        R_Outputfile = std::ptr::null_mut();

        ptr_R_WriteConsole = None;
        ptr_R_WriteConsoleEx = Some(r_write_console);
        ptr_R_ReadConsole = Some(r_read_console);
        ptr_R_ShowMessage = Some(r_show_message);
        ptr_R_Busy = Some(r_busy);

        // Listen for polled events
        R_wait_usec = 10000;
        R_PolledEvents = Some(r_polled_events);

        // Set up main loop
        setup_Rmainloop();

        // Register embedded routines
        r_register_routines();

        // Initialize support functions (after routine registration)
        let r_module_info = modules::initialize().unwrap();

        // TODO: Should starting the R help server proxy really be here?
        // Are we sure we want our own server when ark runs in a Jupyter notebook?
        // Moving this requires detangling `help_server_port` from
        // `modules::initialize()`, which seems doable.
        // Start R help server proxy
        help_proxy::start(r_module_info.help_server_port);

        // Set up the global error handler (after support function initialization)
        errors::initialize();

        // Run the main loop -- does not return
        run_Rmainloop();
    }
}

fn handle_r_request(req: &Request, prompt_recv: &Receiver<PromptInfo>) {
    // Service the request.
    let mutex = unsafe { KERNEL.as_ref().unwrap() };
    {
        let mut kernel = mutex.lock().unwrap();
        kernel.fulfill_request(&req)
    }

    // If this is an execution request, complete it by waiting for R to prompt
    // us before we process another request
    if let Request::ExecuteCode(_, _, _) = req {
        complete_execute_request(req, prompt_recv);
    }
}

fn complete_execute_request(req: &Request, prompt_recv: &Receiver<PromptInfo>) {
    let mutex = unsafe { KERNEL.as_ref().unwrap() };

    // Wait for R to prompt us again. This signals that the
    // execution is finished and R is ready for input again.
    trace!("Waiting for R prompt signaling completion of execution...");
    let prompt_info = prompt_recv.recv().unwrap();
    let prompt = prompt_info.prompt;
    let kernel = mutex.lock().unwrap();

    // Signal prompt
    EVENTS.console_prompt.emit(());

    if prompt_info.incomplete {
        return kernel.report_incomplete_request(&req);
    }

    if prompt_info.user_request {
        if let Request::ExecuteCode(_, originator, _) = req {
            kernel.request_input(originator.clone(), &prompt);
        } else {
            warn!("No originator for input request, omitting");
            kernel.request_input(None, &prompt);
        }

        trace!("Input requested, waiting for reply...");
        return;
    }

    // Default prompt, finishing request
    trace!("Got R prompt '{}', completing execution", prompt);
    return kernel.finish_request();
}

pub fn listen(exec_recv: Receiver<Request>, prompt_recv: Receiver<PromptInfo>) {
    // Before accepting execution requests from the front end, wait for R to
    // prompt us for input.
    trace!("Waiting for R's initial input prompt...");
    let info = prompt_recv.recv().unwrap();
    trace!(
        "Got initial R prompt '{}', ready for execution requests",
        info.prompt
    );

    // Mark kernel as initialized as soon as we get the first input prompt from R
    let mutex = unsafe { KERNEL.as_ref().unwrap() };
    {
        let mut kernel = mutex.lock().unwrap();
        kernel.complete_initialization();
    }

    loop {
        // Wait for an execution request from the front end.
        match exec_recv.recv() {
            Ok(req) => handle_r_request(&req, &prompt_recv),
            Err(err) => warn!("Could not receive execution request from kernel: {}", err),
        }
    }
}
