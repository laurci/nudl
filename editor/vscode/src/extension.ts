import * as vscode from 'vscode';
import {
	LanguageClient,
	LanguageClientOptions,
	ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;

function createClient(): LanguageClient {
	const config = vscode.workspace.getConfiguration('nudl.lsp');
	const configPath = config.get<string>('serverPath', '');
	const serverCommand = configPath || 'nudl-lsp';

	const stdPath = config.get<string>('stdPath', '');
	const env: Record<string, string> = { ...process.env as Record<string, string> };
	if (stdPath) {
		env['NUDL_STD_PATH'] = stdPath;
	}

	const serverOptions: ServerOptions = {
		command: serverCommand,
		args: [],
		options: { env },
	};

	const clientOptions: LanguageClientOptions = {
		documentSelector: [{ scheme: 'file', language: 'nudl' }],
	};

	return new LanguageClient(
		'nudl-lsp',
		'nudl Language Server',
		serverOptions,
		clientOptions,
	);
}

export function activate(context: vscode.ExtensionContext) {
	client = createClient();
	client.start();

	context.subscriptions.push(
		vscode.commands.registerCommand('nudl.restartLsp', async () => {
			if (client) {
				await client.stop();
			}
			client = createClient();
			await client.start();
		}),
	);
}

export async function deactivate() {
	if (client) {
		await client.stop();
	}
}
