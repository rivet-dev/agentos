"use client";

import { motion } from "framer-motion";
import { MessageCircle } from "lucide-react";

const footer = {
  product: [
    { name: "Use Cases", href: "/use-cases" },
    { name: "Pricing", href: "/pricing" },
    { name: "Registry", href: "/registry" },
  ],
  developers: [
    { name: "Documentation", href: "/docs" },
    { name: "Changelog", href: "https://github.com/rivet-dev/agent-os/releases" },
    { name: "GitHub", href: "https://github.com/rivet-dev/agent-os" },
  ],
  legal: [
    { name: "Terms", href: "https://rivet.dev/terms" },
    { name: "Privacy Policy", href: "https://rivet.dev/privacy" },
    { name: "Acceptable Use", href: "https://rivet.dev/acceptable-use" },
  ],
  social: [
    {
      name: "Discord",
      href: "https://rivet.dev/discord",
      icon: <MessageCircle className="h-5 w-5" />,
    },
    {
      name: "GitHub",
      href: "https://github.com/rivet-dev/agent-os",
      icon: (
        <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z" />
        </svg>
      ),
    },
    {
      name: "Twitter",
      href: "https://x.com/rivet_dev",
      icon: (
        <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
          <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
        </svg>
      ),
    },
  ],
};

export function Footer() {
  return (
    <footer className="border-t border-ink/10 bg-paper">
      <div className="mx-auto max-w-6xl px-6 py-16 lg:py-20">
        <div className="xl:grid xl:grid-cols-12 xl:gap-16">
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ duration: 0.5 }}
            className="space-y-6 xl:col-span-4"
          >
            <a href="/" className="inline-block">
              <img src="/images/agent-os/agentos-logo.svg" alt="Agent OS" className="h-7 w-auto" />
            </a>
            <p className="text-sm leading-6 text-ink-soft">A portable open-source operating system for agents.</p>
            <div className="flex space-x-4">
              {footer.social.map((item) => (
                <a
                  key={item.name}
                  href={item.href}
                  className="text-ink-faint transition-colors hover:text-ink"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <span className="sr-only">{item.name}</span>
                  {item.icon}
                </a>
              ))}
            </div>
          </motion.div>

          <div className="mt-12 grid grid-cols-2 gap-8 md:grid-cols-3 xl:col-span-8 xl:mt-0">
            <motion.div initial={{ opacity: 0, y: 20 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ duration: 0.5, delay: 0.1 }}>
              <h3 className="text-sm font-semibold leading-6 text-ink">Product</h3>
              <ul role="list" className="mt-4 space-y-3">
                {footer.product.map((item) => (
                  <li key={item.name}>
                    <a href={item.href} className="text-sm leading-6 text-ink-soft transition-colors hover:text-ink">
                      {item.name}
                    </a>
                  </li>
                ))}
              </ul>
            </motion.div>

            <motion.div initial={{ opacity: 0, y: 20 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ duration: 0.5, delay: 0.15 }}>
              <h3 className="text-sm font-semibold leading-6 text-ink">Developers</h3>
              <ul role="list" className="mt-4 space-y-3">
                {footer.developers.map((item) => (
                  <li key={item.name}>
                    <a href={item.href} className="text-sm leading-6 text-ink-soft transition-colors hover:text-ink">
                      {item.name}
                    </a>
                  </li>
                ))}
              </ul>
            </motion.div>

            <motion.div initial={{ opacity: 0, y: 20 }} whileInView={{ opacity: 1, y: 0 }} viewport={{ once: true }} transition={{ duration: 0.5, delay: 0.2 }}>
              <h3 className="text-sm font-semibold leading-6 text-ink">Legal</h3>
              <ul role="list" className="mt-4 space-y-3">
                {footer.legal.map((item) => (
                  <li key={item.name}>
                    <a href={item.href} className="text-sm leading-6 text-ink-soft transition-colors hover:text-ink">
                      {item.name}
                    </a>
                  </li>
                ))}
              </ul>
            </motion.div>
          </div>
        </div>

        <motion.div
          initial={{ opacity: 0 }}
          whileInView={{ opacity: 1 }}
          viewport={{ once: true }}
          transition={{ duration: 0.5, delay: 0.3 }}
          className="mt-12 border-t border-ink/10 pt-8"
        >
          <p className="text-center text-xs text-ink-faint">&copy; {new Date().getFullYear()} Agent OS. Apache 2.0 licensed.</p>
        </motion.div>
      </div>
    </footer>
  );
}
