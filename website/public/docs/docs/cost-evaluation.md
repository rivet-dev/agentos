# Cost Evaluation

How agentOS compares on cost to per-second sandbox providers when you run coding-agent VMs on your own hardware.

agentOS is a library you run on hardware you already control, not a metered service. That changes the cost model for running coding-agent VMs from "pay a provider per sandbox-second" to "pay for the compute you provision, then pack as much work onto it as it can hold." This page explains where the savings come from and how to reason about them honestly. It does not publish a single magic multiplier, because the real number depends on your workload, your hardware, and how you share VMs.

<Note>For measured latency (cold start, warm execution, and reuse fast paths), see [Benchmarks](/docs/benchmarks). This page is about cost structure, not raw performance.</Note>

## Where the savings come from

Two structural differences drive the cost gap versus per-second sandbox providers:

- **You run on your own hardware**: you choose the cloud, instance type, architecture, and region. A small commodity instance (for example an ARM VM from a budget host) costs a flat hourly or monthly rate that is typically far below what per-sandbox-second billing adds up to once you have steady agent traffic. You also avoid egress fees and vendor lock-in.
- **You decide the isolation granularity**: sandbox providers bill a full container or microVM per execution, usually with a minimum memory reservation that you pay for even when your code uses a fraction of it. With agentOS you own the VM lifecycle: you can dedicate a VM per tenant or per task for maximum isolation, or amortize setup by reusing one VM across many runs.

## The isolation model matters for cost

Each `AgentOs.create()` boots a fully virtualized VM, and each `exec()` / `run()` inside it is a fresh guest process. That gives you a dial between isolation and density:

- **One VM per task or tenant (strongest isolation)**: create a VM, run the work, and dispose it, or give each tenant its own VM. Each VM is its own crash and resource domain, with the highest per-VM overhead. Best when load is untrusted or bursty.
- **A shared VM for trusted work**: reuse one VM across many runs to amortize the VM boot cost. Each `exec()` / `run()` still executes in a fresh guest process, so in-memory state does not leak between runs, but the VM and filesystem are shared. Good for trusted, sequential work.

The denser you can safely pack agent work onto an instance, the lower your effective cost per execution. See [Resource Limits](/docs/resource-limits) for the per-VM caps that govern how densely you can pack, and [Processes & Shell](/docs/processes) for how guest processes run inside a VM.

## How to estimate your own cost

Because agentOS runs on hardware you provision, the honest way to compare is to plug in your own numbers:

1. **Pick your hardware and its rate**: take the hourly or monthly price of the instance you would run on, and divide down to a per-second instance cost.
2. **Estimate how many concurrent VMs fit**: measure per-VM memory overhead on your target hardware under your isolation strategy, then divide your usable RAM by that figure. Leave headroom (the measurement and any orchestration layer will not bin-pack perfectly).
3. **Divide instance cost by concurrent VMs**: that gives a cost-per-VM-second you can compare against a provider's per-sandbox-second rate.

<Tip>Measure on the hardware and isolation strategy you will actually deploy. Per-VM overhead depends on whether you create a fresh VM per task or reuse one across runs, and on the work the agent does, so a number measured on one machine will not transfer cleanly to another.</Tip>

## Comparing against sandbox providers

When you do compare against a per-second sandbox provider, hold the methodology honest:

- **Sandbox cost** is the provider's minimum allocatable memory times their per-GiB-second rate (plus any egress and platform fees). The minimum reservation is the floor you pay even for tiny workloads.
- **agentOS cost** is your instance cost per second divided by the number of VMs you can keep live on it, with realistic headroom for bin-packing inefficiency.

The advantage is largest for **many small, short executions**, where a per-sandbox minimum reservation dominates and your own hardware lets you pack densely. It narrows for **heavyweight, long-lived workloads** (for example dev servers that need hundreds of megabytes regardless), where the win shifts from density to hardware choice: you still avoid per-second metering, egress fees, and lock-in, but the raw memory-density advantage is smaller.

| Workload | Primary cost advantage |
| --- | --- |
| Many small, short executions | Density: pack many VMs per instance, no per-sandbox minimum |
| Heavyweight, long-lived workloads | Hardware choice, flat instance pricing, no egress or lock-in |
| High concurrency | Reuse a VM across runs to amortize VM boot cost |

<Warning>Be careful generalizing cost ratios from a single benchmark. Provider pricing, instance pricing, and exchange rates change over time, and per-VM overhead varies by workload and isolation strategy. Re-measure on your own hardware before quoting a number.</Warning>

When you do need a full Linux sandbox for heavier agent workloads, see [agentOS vs Sandbox](/docs/versus-sandbox) for how the two models combine.