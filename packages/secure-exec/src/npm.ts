import type {
	CodeExecutionResult,
	LanguageExecutionOptions,
	NpmPackageInstallOptions,
	NpmProjectInstallOptions,
} from "@rivet-dev/agentos-core";
import { type Context, contextVm } from "./runtime.js";

// npm operations change a VM's filesystem, so they only make sense in a
// context, whose VM outlives the call.
type InContext<O> = Omit<O, "contextId"> & { context: Context };

export type InstallProjectOptions = InContext<NpmProjectInstallOptions>;
export type InstallPackagesOptions = InContext<NpmPackageInstallOptions>;
export type RunScriptOptions = InContext<LanguageExecutionOptions>;
export type RunPackageOptions = InContext<
	LanguageExecutionOptions & { binary?: string }
>;

/** Install the dependencies of the `package.json` in the context's working directory. */
export function install(
	options: InstallProjectOptions,
): Promise<CodeExecutionResult>;
/** Install packages into the context's VM. */
export function install(
	packages: string | string[],
	options: InstallPackagesOptions,
): Promise<CodeExecutionResult>;
export function install(
	packagesOrOptions: string | string[] | InstallProjectOptions,
	packageOptions?: InstallPackagesOptions,
): Promise<CodeExecutionResult> {
	if (
		typeof packagesOrOptions === "string" ||
		Array.isArray(packagesOrOptions)
	) {
		const { context, ...options } = requireContext(packageOptions);
		return contextVm(context).javascript.npm.install(
			packagesOrOptions,
			options,
		);
	}
	const { context, ...options } = requireContext(packagesOrOptions);
	return contextVm(context).javascript.npm.install(options);
}

/** Run a `package.json` script, like `npm run`. */
export function runScript(
	script: string,
	options: RunScriptOptions,
): Promise<CodeExecutionResult> {
	const { context, ...rest } = requireContext(options);
	return contextVm(context).javascript.npm.runScript(script, rest);
}

/** Run a package's binary, like `npx`. */
export function runPackage(
	packageSpec: string,
	options: RunPackageOptions,
): Promise<CodeExecutionResult> {
	const { context, ...rest } = requireContext(options);
	return contextVm(context).javascript.npm.runPackage(packageSpec, rest);
}

function requireContext<O extends { context: Context }>(
	options: O | undefined,
): O {
	if (!options?.context) {
		throw new TypeError(
			"npm operations require a context; create one with createContext()",
		);
	}
	return options;
}
