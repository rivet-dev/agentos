import type { RegistryIconName } from "../../../data/registry-icons";
import { REGISTRY_ICONS } from "../../../data/registry-icons";

// The App Store-style squircle icon tile, shared by the registry storefront
// (inside the React island) and the static detail-page header. Hook-free so
// zero-JS Astro pages can render it server-side, and prop types stay plain
// serializable values so it also works across the island boundary.

// SVGs with a baked full-bleed background plate render edge-to-edge so the
// tile radius clips them; padding these would show a hard-cornered colored
// square floating inside the tile.
const BLEED_IMAGES = new Set([
	"/images/registry/browserbase.svg",
	"/images/registry/opencode.svg",
]);

// Wide wordmark logos get width-fit rendering; contained in a square at the
// default scale they shrink to thin strips.
const WIDE_IMAGES = new Set([
	"/images/registry/e2b.svg",
	"/images/registry/jq.svg",
	"/images/registry/modal.svg",
	"/images/registry/super-memory.svg",
]);

// Monogram washes for entries with no artwork, picked deterministically from
// the title so a shelf of them reads as a designed set, not repeated
// placeholders.
const MONOGRAM_TINTS = [
	"bg-pine/[0.08] text-pine",
	"bg-olive/10 text-olive",
	"bg-sage/25 text-pine",
] as const;

const TILE_SURFACE =
	"relative flex shrink-0 items-center justify-center overflow-hidden " +
	"ring-1 ring-ink/[0.08] " +
	"shadow-[inset_0_1px_0_rgba(255,255,255,0.9),0_1px_2px_-1px_rgba(20,20,22,0.10)]";

function monogramTint(title: string) {
	let hash = 0;
	for (const char of title) hash = (hash + char.charCodeAt(0)) % 997;
	return MONOGRAM_TINTS[hash % MONOGRAM_TINTS.length];
}

export interface RegistryIconTileProps {
	title: string;
	image?: string;
	icon?: RegistryIconName;
	size: number;
	className?: string;
}

export function RegistryIconTile({
	title,
	image,
	icon,
	size,
	className,
}: RegistryIconTileProps) {
	const frame = {
		width: size,
		height: size,
		borderRadius: Math.round(size * 0.24),
	};

	if (image) {
		if (BLEED_IMAGES.has(image)) {
			return (
				<div className={`${TILE_SURFACE} ${className ?? ""}`} style={frame}>
					<img
						src={image}
						alt=""
						className="h-full w-full object-cover"
						width={size}
						height={size}
					/>
				</div>
			);
		}
		return (
			<div
				className={`${TILE_SURFACE} bg-gradient-to-b from-white to-[#f9f9fa] ${className ?? ""}`}
				style={frame}
			>
				<img
					src={image}
					alt=""
					className="object-contain"
					style={
						WIDE_IMAGES.has(image)
							? { width: size * 0.75, height: "auto" }
							: { width: size * 0.62, height: size * 0.62 }
					}
				/>
			</div>
		);
	}

	if (icon) {
		const IconComponent = REGISTRY_ICONS[icon];
		return (
			<div
				className={`${TILE_SURFACE} bg-gradient-to-b from-white to-[#f9f9fa] ${className ?? ""}`}
				style={frame}
			>
				<IconComponent
					className="text-ink-soft"
					style={{ width: size * 0.5, height: size * 0.5 }}
				/>
			</div>
		);
	}

	return (
		<div
			className={`${TILE_SURFACE} ${monogramTint(title)} ${className ?? ""}`}
			style={frame}
		>
			<span
				className="select-none font-semibold uppercase"
				style={{
					fontSize: size * 0.4,
					lineHeight: 1,
					// Manrope capitals paint slightly low-right of their em box
					// (measured via canvas actualBoundingBox); nudge the ink onto
					// the tile's true center.
					transform: "translate(-0.015em, -0.03em)",
				}}
			>
				{title.charAt(0)}
			</span>
		</div>
	);
}
