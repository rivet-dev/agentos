export function resolveRelativeNonFileUrl(raw, baseUrl, posixPath) {
	const basePathname = baseUrl.pathname || "/";
	const origin = `${baseUrl.protocol}//${baseUrl.host}`;
	if (raw.startsWith("//")) {
		return `${baseUrl.protocol}${raw}`;
	}
	if (raw.startsWith("#")) {
		return `${origin}${basePathname}${baseUrl.search || ""}${raw}`;
	}
	if (raw.startsWith("?")) {
		return `${origin}${basePathname}${raw}`;
	}
	if (raw === "") {
		return `${origin}${basePathname}${baseUrl.search || ""}`;
	}
	const queryIndex = raw.indexOf("?");
	const hashIndex = raw.indexOf("#");
	const searchStart = queryIndex === -1 ? raw.length : queryIndex;
	const hashStart = hashIndex === -1 ? raw.length : hashIndex;
	const pathEnd = Math.min(searchStart, hashStart);
	const relativePath = raw.slice(0, pathEnd);
	const suffix = raw.slice(pathEnd);
	const baseDirectory = basePathname.endsWith("/")
		? basePathname
		: posixPath.dirname(basePathname);
	let resolvedPath = relativePath.startsWith("/")
		? posixPath.resolve("/", relativePath)
		: posixPath.resolve(baseDirectory, relativePath);
	if (
		(relativePath.endsWith("/") || /(^|\/)\.\.?$/.test(relativePath)) &&
		!resolvedPath.endsWith("/")
	) {
		resolvedPath += "/";
	}
	return `${origin}${resolvedPath}${suffix}`;
}
