# Amalthea

## About

Experimental kernel framework and R kernel for Jupyter and Positron, written in Rust.

![image](https://user-images.githubusercontent.com/470418/151626974-52ac0047-0e98-494d-ad00-c0d293df696f.png)

This repository contains five individual projects, which are evolving together.

- **Amalthea**, a Rust framework for building Jupyter and Positron kernels.
- **ARK**, the Amalthea R Kernel. ARK is a native kernel for R built on the Amalthea framework that interacts with the R interpreter in the same way RStudio does (it's a real front end). It also implements the Language Server Protocol, using [tower-lsp](https://github.com/ebkalderon/tower-lsp).
- **echo**, a toy kernel for a fictional language that can be used to experiment with the kernel framework without the nuisance of getting language bindings working. As the name implies, it is a language that just echoes its input back as output.
- **harp**, safe Rust wrappers for R objects and interfaces.
- **stdext**, extensions to Rust's standard library for utility use in the other four projects.

```mermaid
flowchart TD
a[Amalthea] <--Message Handlers--> ark(((Amalthea R Kernel - ark)))
ark <--> lsp[Language Protocol Server]
ark <--> h[harp R wrapper]
ark <--> libr[libR-sys bindings]
h <--> libr
libr <--> r[R Shared Library]
lsp <--> h
lsp <--> libr
lsp <--> tower[Tower-LSP]
```

For more information on the system's architecture, see the [Amalthea Architecture](https://connect.rstudioservices.com/positron-wiki/amalthea-architecture.html) section of the Positron Wiki.

### What's with the name?

This is a Jupyter kernel framework; Amalthea is [one of Jupiter's moons](https://en.wikipedia.org/wiki/Amalthea_(moon)).

### Amalthea R Kernel Installation/Usage

Install Rust. If you don't already have it, use `rustup`, following the [installation instructions at rustup.rs](https://rustup.rs/). In brief:

```bash
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
$ source $HOME/.cargo/env
```

Assuming you have a working Rust toolchain, first build the sources.

```bash
$ cargo build
```
Next, install the kernelspec. From the repository root:

```bash
$ ./target/debug/ark --install
```

This installs a JSON file to the Jupyter kernel registry. After it completes, the Amalthea R kernel (ARK) will be available on all Jupyter frontends on your system (Notebook, Lab, Positron, etc.).

You will usually want to tweak the **ark** environment for development; add this to `~/Library/Jupyter/kernels/ark/kernel.json`:

```json
  "env": {
    "RUST_LOG": "trace",
    "R_HOME": "/Library/Frameworks/R.framework/Resources"
  }
```

More fine-grained control of logging is available for `RUST_LOG` as documented in [env_logger](https://docs.rs/env_logger/0.9.0/env_logger/#enabling-logging).

## Dev Workflow: VS Code LSP

### Setup

1. Build and install **ark** using the instructions above. Use Jupyter to verify that your kernel works!
2. Build and install the [myriac-console extension](https://github.com/rstudio/myriac-console); use `vsce package` and then install the `.VSIX` file.
3. Build and install the [ark extension](https://github.com/rstudio/amalthea/tree/main/ark/extension).
4. Open the settings UI in VS Code and search for `ark`. Set `Ark > Trace: Server` to `verbose`.

### Development

1. Open a new VS Code window.
2. Run the "New Myriac Console" command and start the **ark** kernel. Eval some test expressions to validate that connectivity to R is working.
3. Open a `.R` file.

This will cause the following things to happen:

1. The **ark** extension will activate, since it is registered for `.R` files.
2. It will locate the **myriac-console** extension and ask it to start an LSP for R.
3. The **myriac-console** extension will send a Jupyter message to the **ark** kernel, asking it to start its LSP.
4. The **ark** kernel will start the LSP and connect to the language client provided by the **ark** extension.

Once everything is running, check VS Code's _Output_ tab in the bottom panel. You will see the following entries:

`Amalthea R Kernel` -- Debug output from the kernel. This will be pretty verbose presuming you've set `RUST_LOG` to `trace` as recommended above.

`ARK Language Server` -- Output that was sent from the language server to the client.

`ARK Language Server (Trace)` -- Assuming you've set the server to use verbose output, this is the most interesting log for LSP development; it will show a complete log of all client/server interactions.

## Related Projects

[Xeus](https://github.com/jupyter-xeus/xeus), a C++ base/reference kernel implementation

[IRKernel](https://github.com/IRkernel/IRkernel), a kernel for R written primarily in R itself

[EvCxR Kernel](https://github.com/google/evcxr/tree/main/evcxr_jupyter), a kernel for Rust written in Rust

[Myriac Console](https://github.com/rstudio/myriac-console), an experimental Jupyter front end

[tower-lsp](https://github.com/ebkalderon/tower-lsp), an LSP framework built on [Tower](https://github.com/tower-rs/tower), which is itself built on [tokio](https://tokio.rs/).

[tower-lsp-boilerplate](https://github.com/IWANABETHATGUY/tower-lsp-boilerplate), an example LSP built with `tower-lsp`
