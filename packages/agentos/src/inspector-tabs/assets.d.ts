// Vite emits imported SVGs as asset URLs in the tabs bundle.
declare module "*.svg" {
	const src: string;
	export default src;
}
