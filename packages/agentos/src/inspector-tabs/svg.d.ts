// Vite emits imported SVGs as hashed asset files; the default export is the
// URL. (tsconfig has "types": [], so vite/client is not in scope.)
declare module "*.svg" {
	const url: string;
	export default url;
}
