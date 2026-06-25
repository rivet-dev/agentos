"use client";

import { useState, useEffect } from "react";
import { Menu, X } from "lucide-react";
import { GitHubStars } from "./GitHubStars";
import { registry } from "../data/registry";

function DiscordIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      fill="currentColor"
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <path d="M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z" />
    </svg>
  );
}

const NAV_LINKS: { href: string; label: string; badge?: number }[] = [
  { href: "/docs", label: "Documentation" },
  { href: "/use-cases", label: "Use Cases" },
  { href: "/registry", label: "Registry", badge: registry.length },
  { href: "/docs/deployment", label: "Deploy" },
];

function NavBadge({ count }: { count: number }) {
  return (
    <span className="inline-flex items-center rounded-full bg-ink/[0.06] px-1.5 py-0.5 text-[11px] font-medium tabular-nums text-ink-faint">
      {count}
    </span>
  );
}

function NavItem({ href, children, badge }: { href: string; children: React.ReactNode; badge?: number }) {
  return (
    <a
      href={href}
      className="inline-flex items-center gap-1.5 px-3 py-2 text-sm font-medium text-ink-soft transition-colors duration-200 hover:text-ink"
    >
      {children}
      {badge != null && <NavBadge count={badge} />}
    </a>
  );
}

export function Navigation({ revealLogoOnScroll = false }: { revealLogoOnScroll?: boolean }) {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);
  // On pages with a hero logo, keep the nav logo hidden until the hero logo
  // scrolls up behind the nav. Elsewhere it's always visible.
  const [logoVisible, setLogoVisible] = useState(!revealLogoOnScroll);

  useEffect(() => {
    const handleScroll = () => setIsScrolled(window.scrollY > 20);
    handleScroll();
    window.addEventListener("scroll", handleScroll);
    return () => window.removeEventListener("scroll", handleScroll);
  }, []);

  useEffect(() => {
    if (!revealLogoOnScroll) return;
    const heroLogo = document.getElementById("hero-logo");
    if (!heroLogo) {
      setLogoVisible(true); // fail open: no hero logo on this page → always show
      return;
    }
    const observer = new IntersectionObserver(
      ([entry]) => setLogoVisible(!entry.isIntersecting),
      // Negative top margin (~nav height) so the crossover lands at the nav
      // rather than the very top edge of the viewport.
      { rootMargin: "-80px 0px 0px 0px" },
    );
    observer.observe(heroLogo);
    return () => observer.disconnect();
  }, [revealLogoOnScroll]);

  return (
    <div className="fixed top-0 z-50 w-full md:left-1/2 md:top-4 md:w-full md:max-w-[1200px] md:-translate-x-1/2 md:px-8">
      <div className="relative">
        <div
          className={`absolute inset-0 -z-[1] hidden overflow-hidden rounded-xl border transition-all duration-300 ease-in-out md:block ${
            isScrolled
              ? "border-ink/10 bg-paper/80 backdrop-blur-lg"
              : "border-transparent bg-transparent backdrop-blur-none"
          }`}
        />

        <header
          className={`sticky top-0 z-10 flex flex-col items-center border-b bg-paper/85 pb-2 pt-2 backdrop-blur-md transition-all md:static md:rounded-xl md:border-transparent md:bg-transparent md:backdrop-blur-none ${
            isScrolled ? "border-ink/10" : "border-transparent"
          }`}
        >
          <div className="flex w-full items-center justify-between px-3">
            <div className="flex items-center">
              {/* Collapsing logo cell — the nav links slide right as it expands
                  in and left as it collapses out, so the row stays balanced. */}
              <div
                className={`grid transition-all duration-300 ease-out ${
                  logoVisible ? "grid-cols-[1fr]" : "grid-cols-[0fr]"
                }`}
              >
                <div className="overflow-hidden">
                  <a
                    href="/"
                    aria-hidden={!logoVisible}
                    tabIndex={logoVisible ? undefined : -1}
                    className={`flex items-center pr-6 transition-all duration-300 ease-out ${
                      logoVisible
                        ? "opacity-100 blur-0"
                        : "pointer-events-none opacity-0 blur-sm"
                    }`}
                  >
                    <img
                      src="/images/agent-os/agentos-hero-logo.svg"
                      alt="agentOS"
                      className="h-7 w-auto max-w-none"
                    />
                  </a>
                </div>
              </div>

              <div className="hidden items-center md:flex">
                {NAV_LINKS.map((link) => (
                  <NavItem key={link.href} href={link.href} badge={link.badge}>
                    {link.label}
                  </NavItem>
                ))}
              </div>
            </div>

            <div className="hidden flex-row items-center gap-2 md:flex">
              <a
                href="https://rivet.dev/discord"
                className="inline-flex h-10 items-center justify-center whitespace-nowrap rounded-md border border-ink/15 px-4 py-2 text-sm text-ink-soft transition-colors hover:border-ink/30 hover:text-ink"
                aria-label="Discord"
              >
                <DiscordIcon className="h-5 w-5" />
              </a>
              <GitHubStars
                repo="rivet-dev/agent-os"
                className="inline-flex h-10 items-center justify-center gap-2 whitespace-nowrap rounded-md border border-ink/15 bg-white/55 px-4 py-2 text-sm text-ink shadow-sm transition-colors hover:border-ink/30"
              />
            </div>

            <button
              className="p-2 text-ink-soft transition-colors hover:text-ink md:hidden"
              onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
              aria-label="Toggle menu"
            >
              {mobileMenuOpen ? <X className="h-6 w-6" /> : <Menu className="h-6 w-6" />}
            </button>
          </div>
        </header>
      </div>

      {mobileMenuOpen && (
        <div className="mx-2 mt-2 rounded-xl border border-ink/10 bg-paper/95 shadow-xl backdrop-blur-lg md:hidden">
          <div className="space-y-1 px-4 py-4">
            {NAV_LINKS.map((link) => (
              <a
                key={link.href}
                href={link.href}
                className="flex items-center gap-2 rounded-lg px-3 py-2.5 font-medium text-ink-soft transition-colors hover:bg-ink/5 hover:text-ink"
                onClick={() => setMobileMenuOpen(false)}
              >
                {link.label}
                {link.badge != null && <NavBadge count={link.badge} />}
              </a>
            ))}
            <div className="mt-3 space-y-1 border-t border-ink/10 pt-3">
              <a
                href="https://rivet.dev/discord"
                className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-ink-soft transition-colors hover:bg-ink/5 hover:text-ink"
                onClick={() => setMobileMenuOpen(false)}
                aria-label="Discord"
              >
                <DiscordIcon className="h-5 w-5" />
                <span className="font-medium">Discord</span>
              </a>
              <GitHubStars
                repo="rivet-dev/agent-os"
                className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-ink-soft transition-colors hover:bg-ink/5 hover:text-ink"
                onClick={() => setMobileMenuOpen(false)}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
