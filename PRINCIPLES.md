# AI GUI Principles

[简体中文](./PRINCIPLES.zh-CN.md) · [Manifesto](./MANIFESTO.md) ·
[Specification map](./SPEC.md)

This document turns the values of the AI GUI Manifesto into practical design and engineering
checks. It is a companion to the manifesto, not a replacement for the PodApp Protocol.

## Choose the interaction from the task

Use three questions before adding either a chat flow or a GUI:

1. **How ambiguous is the intent?** Open-ended goals benefit from language and AI reasoning.
2. **How consequential is the action?** Irreversible, external, costly, or sensitive actions need
   explicit scope and confirmation.
3. **How repeatable is the interaction?** A frequently repeated choice or transformation should
   become a stable action with a reusable interface.

The result may be chat, a Pod, or a sequence that moves between both. The goal is not fewer
controls at any cost. The goal is the smallest interaction that makes the action understood.

## Ten implementation principles

### 1. Give every meaningful action a stable identity

A business action has a stable Action ID, versioned input and output schemas, declared effects, and
documented failure states. UI events such as opening a tab or dragging a window are not business
actions.

**Evidence:** an action contract can be inspected without launching the GUI.

### 2. Implement the action once

GUI, CLI, MCP, API, tests, and agent calls invoke the same headless action core. A surface adapts
input and presents state; it does not reimplement the business behavior.

**Evidence:** changing the action does not require matching edits in several surfaces.

### 3. Make the proposed action inspectable

Before commitment, show the target, scope, important inputs, expected effects, and assumptions.
Confirmation text must name the actual action rather than ask a context-free "Are you sure?"

**Evidence:** a person can explain what will happen from the confirmation view alone.

### 4. Expose state and uncertainty

Represent planned, waiting, running, succeeded, failed, cancelled, and conflicted states
explicitly. Distinguish user-provided values from model-inferred values. Do not present a guess as a
validated fact.

**Evidence:** the current state and source of critical values are machine-readable.

### 5. Put permission enforcement below every surface

Permissions are declared narrowly and enforced by the action core or its host, not by the
visibility of a GUI button. Bypassing the surface must never bypass authorization.

**Evidence:** the same denied call fails through GUI, headless, CLI, and MCP paths.

### 6. Design for recovery before autonomy

Prefer preview, draft, versioning, idempotency, undo, and rollback. Where an action cannot be
reversed, make that fact visible before execution and minimize the affected scope.

**Evidence:** tests cover retries, partial failures, stale state, and recovery paths.

### 7. Keep human control reachable

Long-running work supports cancellation or safe interruption. People can edit assumptions, reject
a proposal, retry a failed step, and take over manually without losing the work already completed.

**Evidence:** control is available while work is running, not only before it starts.

### 8. Keep Pods small and composable

A Pod owns one coherent action or a tightly related action family. Inputs, outputs, artifacts,
events, and state use structured contracts so that another Pod or an AI system can connect to it
without screen scraping.

**Evidence:** a Pod can be invoked headlessly and its output can become another action's input.

### 9. Treat accessibility as part of the machine interface

Controls have stable non-visual identifiers, names, roles, values, and keyboard behavior. This
helps people using assistive technology and gives automation a reliable alternative to pixels.

**Evidence:** the important path can be understood through the accessibility tree.

### 10. Verify behavior at the right layer

Test business behavior through the action core, surface bindings through contract checks, remote
and retry behavior through recorded envelopes, and human comprehension through a small number of
UI and accessibility tests. Screenshots are evidence of appearance, not business correctness.

**Evidence:** each failure can be localized to action, binding, transport, or presentation.

## A practical release gate

Before publishing a Pod or AI GUI capability, confirm:

- [ ] The action has a stable ID and schemas for input, output, and effects.
- [ ] Human and machine callers share one implementation path.
- [ ] Inferred values and uncertainty are visible where they matter.
- [ ] Consequential actions show target, scope, and effects before commitment.
- [ ] Permissions are minimal, declared, and enforced below the UI.
- [ ] Running work can be observed and, where safe, interrupted.
- [ ] Retry, stale state, partial failure, and recovery have defined behavior.
- [ ] Outputs are structured or returned as artifact references, not hidden in a screen.
- [ ] Important controls have stable accessible identifiers.
- [ ] Tests validate the action directly and the delivered artifact on a clean environment.

These checks are intentionally stricter for actions that spend money, communicate externally,
modify shared state, grant access, or cannot be undone.

---

The manifesto describes the direction. These principles describe the design discipline. Normative
package and action contracts are listed in [SPEC.md](./SPEC.md).
