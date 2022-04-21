import * as vscode from 'vscode';
import * as path from 'path';

import {
	LanguageClient,
	LanguageClientOptions,
	ServerOptions,
	TransportKind,
	createClientSocketTransport
} from 'vscode-languageclient/node';

let client: LanguageClient;
export function activate(context: vscode.ExtensionContext) {

	console.log('Activating ARK language server extension');

	let disposable = vscode.commands.registerCommand('ark.helloWorld', () => {
		vscode.window.showInformationMessage('Hello World from ark!');
	});

	context.subscriptions.push(disposable);

	// Locate the Myriac Console extension, which supplies the other side of the language server.
	let ext = vscode.extensions.getExtension("RStudio.myriac-console");
	if (!ext) {
		vscode.window.showErrorMessage("Could not find Myriac Console extension; please install it.\n\n" +
			"R language server will not be available.");
		return null;
	}

	let serverOptions = () => {
		// Find an open port for the language server to listen on.
		var portfinder = require('portfinder');
		console.info('Finding open port for R language server...');
		let stream = portfinder.getPortPromise()
			.then(async (port: number) => {
				let address = `127.0.0.1:${port}`;
				try {
					// Create our own socket transport
					const transport = await createClientSocketTransport(port);

					// Ask Myriac to start the language server
					console.log(`Requesting Myriac Console extension to start R language server at ${address}...`);
					ext?.exports.startLsp("R", address);
					// TODO: Need to handle errors arising from LSP startup.

					// Wait for the language server to connect to us
					console.log(`Waiting to connect to language server at ${address}...`);
					const protocol = await transport.onConnected();
					console.log(`Connected to language server at ${address}, returning protocol transports`);
					return {
						reader: protocol[0],
						writer: protocol[1]
					};
				} catch (err) {
					vscode.window.showErrorMessage("Could not connect to language server: \n\n" + err);
				}
			})
			.catch((err: string) => {
				vscode.window.showErrorMessage("Could not find open port for language server: \n\n" + err);
			});
		return stream;
	};

	// TODO: Only create the output channel if the ark.trace.server option is set.
	let clientOptions: LanguageClientOptions = {
		documentSelector: [{ scheme: 'file', language: 'R' }],
		traceOutputChannel: vscode.window.createOutputChannel('ARK Language Server - Trace')
	};

	console.log('Creating language client');
	client = new LanguageClient('ark', 'ARK Language Server', serverOptions, clientOptions);

	context.subscriptions.push(client.start());
};


export function deactivate() { }
