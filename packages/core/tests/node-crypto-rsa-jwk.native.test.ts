import {
	generateKeyPairSync,
	privateDecrypt,
	publicEncrypt,
} from "node:crypto";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("RSA JWK and CryptoKey imports interoperate with Node OAEP SHA-256 and labels", async () => {
	const pair = generateKeyPairSync("rsa", { modulusLength: 2048 });
	const publicJwk = pair.publicKey.export({ format: "jwk" }),
		privateJwk = pair.privateKey.export({ format: "jwk" });
	const label = Buffer.from("guest interoperability");
	const ciphertext = publicEncrypt(
		{ key: pair.publicKey, oaepHash: "sha256", oaepLabel: label },
		Buffer.from("from host"),
	);
	const source = `import {createPublicKey,createPrivateKey,KeyObject,publicEncrypt,privateDecrypt} from 'node:crypto';
 const pub=${JSON.stringify(publicJwk)},priv=${JSON.stringify(privateJwk)};
 const publicKey=createPublicKey({key:pub,format:'jwk'}),privateKey=createPrivateKey({key:priv,format:'jwk'});
 const options={oaepHash:'sha256',oaepLabel:Buffer.from(${JSON.stringify(label.toString("base64"))},'base64')};
 const clear=privateDecrypt({key:privateKey,...options},Buffer.from(${JSON.stringify(ciphertext.toString("base64"))},'base64')).toString();
 const web=await crypto.subtle.importKey('jwk',pub,{name:'RSA-OAEP',hash:'SHA-256'},true,['encrypt']);
 const result=publicEncrypt({key:KeyObject.from(web),...options},Buffer.from('from guest'));
 let invalid='';try{await crypto.subtle.importKey('jwk',pub,{name:'RSA-OAEP',hash:'SHA-256'},true,['decrypt']);}catch(e){invalid=e.name;}
 let wrong='';try{privateDecrypt({key:privateKey,oaepHash:'sha1'},result);}catch(e){wrong=e.code;}
 console.log(JSON.stringify({clear,cipher:result.toString('base64'),publicRoundTrip:publicKey.export({format:'jwk'}),privateRoundTrip:privateKey.export({format:'jwk'}),invalid,wrong,algorithm:web.algorithm.name,type:web.type}));`;
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn(
			"node",
			["--input-type=module", "-e", source],
			{
				timeoutMs: 10000,
				onStdout: (b) => {
					stdout += Buffer.from(b).toString();
				},
				onStderr: (b) => {
					stderr += Buffer.from(b).toString();
				},
			},
		);
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		const result = JSON.parse(stdout);
		expect(result.clear).toBe("from host");
		expect(result.publicRoundTrip).toEqual(publicJwk);
		expect(result.privateRoundTrip).toEqual(privateJwk);
		expect(result.invalid).toBe("SyntaxError");
		expect(result.wrong).toMatch(/^ERR_OSSL_/);
		expect(result.algorithm).toBe("RSA-OAEP");
		expect(result.type).toBe("public");
		expect(
			privateDecrypt(
				{ key: pair.privateKey, oaepHash: "sha256", oaepLabel: label },
				Buffer.from(result.cipher, "base64"),
			).toString(),
		).toBe("from guest");
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 20000);
