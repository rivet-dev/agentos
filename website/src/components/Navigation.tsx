"use client";

import { useState, useEffect } from "react";
import { Menu, X, MessageCircle } from "lucide-react";
import { GitHubStars } from "./GitHubStars";

const NAV_LINKS = [
  { href: "/use-cases", label: "Use Cases" },
  { href: "/pricing", label: "Pricing" },
  { href: "/registry", label: "Registry" },
  { href: "/docs", label: "Docs" },
];

function NavItem({ href, children }: { href: string; children: React.ReactNode }) {
  return (
    <a
      href={href}
      className="px-3 py-2 text-sm font-medium text-ink-soft transition-colors duration-200 hover:text-ink"
    >
      {children}
    </a>
  );
}

export function Navigation() {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [isScrolled, setIsScrolled] = useState(false);

  useEffect(() => {
    const handleScroll = () => setIsScrolled(window.scrollY > 20);
    handleScroll();
    window.addEventListener("scroll", handleScroll);
    return () => window.removeEventListener("scroll", handleScroll);
  }, []);

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
            <div className="flex items-center gap-4">
              <a href="/" className="flex items-center gap-2">
                <img
                  src="/images/agent-os/agentos-logo.svg"
                  alt="Agent OS"
                  className="h-7 w-auto"
                />
              </a>

              <div className="ml-2 hidden items-center md:flex">
                {NAV_LINKS.map((link) => (
                  <NavItem key={link.href} href={link.href}>
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
                <MessageCircle className="h-5 w-5" />
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
                className="block rounded-lg px-3 py-2.5 font-medium text-ink-soft transition-colors hover:bg-ink/5 hover:text-ink"
                onClick={() => setMobileMenuOpen(false)}
              >
                {link.label}
              </a>
            ))}
            <div className="mt-3 space-y-1 border-t border-ink/10 pt-3">
              <a
                href="https://rivet.dev/discord"
                className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-ink-soft transition-colors hover:bg-ink/5 hover:text-ink"
                onClick={() => setMobileMenuOpen(false)}
                aria-label="Discord"
              >
                <MessageCircle className="h-5 w-5" />
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
