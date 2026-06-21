import type { FaqItem } from './types';

// FAQ content for the agentOS pricing page. Rendered by AgentOSPricingPage
// and emitted as FAQPage JSON-LD from pages/pricing.astro.
export const agentOsPricingFaqs: FaqItem[] = [
	{
		question: 'Is agentOS really free?',
		answerHtml:
			'Yes. agentOS is open source under the Apache 2.0 license. You can run it on your own infrastructure at no cost, forever.',
	},
	{
		question: 'Can I use agentOS in production?',
		answerHtml:
			'Absolutely. agentOS is designed to run in production from your laptop to on-prem clusters. It is just an npm package, so it deploys wherever your code already runs.',
	},
	{
		question: 'What does the Enterprise tier include?',
		answerHtml:
			'Enterprise includes on-premise and air-gapped deployment support, custom SLAs, priority support, custom integrations, and security reviews & compliance assistance.',
	},
	{
		question: 'What support is available for open source users?',
		answerHtml:
			'Open source users can get help through our Discord community and GitHub issues. Enterprise customers receive dedicated support channels with guaranteed response times.',
	},
	{
		question: 'Do you offer volume discounts?',
		answerHtml:
			'Yes. Contact our sales team for custom pricing on high-volume usage or enterprise deployments.',
	},
];
