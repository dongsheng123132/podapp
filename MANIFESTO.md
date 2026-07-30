# The AI GUI Manifesto

## A deterministic interaction layer between human intent and probabilistic intelligence

> **The future is not No GUI. It is Just Enough GUI.**

Software is collapsing into conversation.

Pages, menus, forms, toolbars, and workflows that once demanded dozens of clicks can now be
replaced by a sentence. Instead of learning software, people can describe what they want and let
AI find a path.

This is progress. But it has also produced a dangerous misunderstanding: because interfaces can
be reduced, interfaces should disappear.

We disagree.

**Prediction is not intent.**

**Generation is not confirmation.**

**Capability is not permission.**

**Execution is not a decision.**

Chat is a powerful interface. It is not the interface for every problem.

---

## Language expresses intent. Interfaces resolve ambiguity.

When a goal is open-ended, we should talk with AI.

When a choice must be exact, we should show the choice.

When an action has consequences, we should show its scope and ask for confirmation.

When an interaction will be repeated, we should turn it into a reliable tool.

"Improve this image" belongs in language. The exact crop, protected region, output size, and
placement of a QR code belong in a visible interface.

"Arrange a meeting" belongs in language. The attendees, time zone, final time, and moment of
sending belong in an explicit confirmation.

"Organize these files" belongs in language. Which versions will be replaced, which files will be
deleted, and who will receive access must not be guessed.

Natural language is excellent at opening a space of possibilities. A well-designed GUI is
excellent at closing that space to one understood action.

---

## What is an AI GUI?

An AI GUI is not a chat box added to conventional software.

It is not a model hidden behind an old interface.

It is not more UI for its own sake.

> **AI GUI is the deterministic interaction layer between human intent and probabilistic intelligence.**

It turns ambiguous language into structured input, generated possibilities into visible choices,
and proposed operations into actions that can be inspected, confirmed, interrupted, and
recovered.

"Deterministic" does not mean that AI will never be wrong. It means that the selected objects,
current state, permissions, effects, and final commitment are made explicit and can be verified by
both people and machines.

An AI GUI serves three participants:

- **People** use it to express, select, confirm, correct, and take over.
- **AI** uses it to understand context, constraints, state, and authority.
- **Systems** use it to validate, execute, record, and compose actions.

The GUI is no longer merely a human-facing surface. It becomes a shared control plane for humans
and AI.

---

## What we value

Through building and using AI interfaces, we have come to value:

**Explicit intent** over plausible inference

**Visible state** over invisible automation

**Confirmed and recoverable action** over frictionless execution

**Human authority** over unchecked autonomy

**Small, composable actions** over monolithic applications

**Open, portable contracts** over platform-locked plugins

**One action core with many surfaces** over duplicated implementations

The items on the right still have value. When they conflict, we choose the items on the left.

---

## Twelve principles

### 1. Chat is an interface, not the interface.

Conversation is ideal for goals, preferences, explanations, and ambiguity. It is not sufficient
for every exact selection, state inspection, spatial operation, or consequential confirmation.
The interaction should fit the task, not a trend.

### 2. Do not make AI guess what can be selected.

Dates, amounts, regions, colours, locations, recipients, files, permissions, and execution scope
should become explicit controls when precision matters. The purpose of the control is not to add
clicks; it is to remove ambiguity.

### 3. Uncertainty must be visible.

AI should show what it understood, what it assumed, what information is missing, what it plans to
do, and where confidence is low. Uncertainty is not failure. Hiding uncertainty behind fluent
language is failure.

### 4. Consequential actions require specific confirmation.

Before an irreversible, external, costly, or high-impact action, the interface should show the
target, scope, effects, and important alternatives. A person should confirm a concrete action, not
an abstract button labelled "OK."

### 5. Recovery is a default capability.

Preview, draft, version history, undo, retry, restore, and rollback are part of the core design of
AI software. We do not need systems that pretend never to fail. We need systems that fail safely
and recover clearly.

### 6. System state must remain visible.

People should be able to see what AI is doing, what has completed, what is waiting, which tools and
data are in use, and which permissions are active. Automation must not become an invisible
background authority.

### 7. People must be able to interrupt and take over.

Pause, stop, edit, reject, retry, undo, and switch to manual control are fundamental operations.
AI should carry work when invited and return control when requested.

### 8. Show just enough GUI, at the moment it is needed.

AI GUI is not a return to screens filled with permanent controls. The interface should appear when
selection, spatial reasoning, review, or confirmation becomes valuable, and recede when language
is enough.

### 9. The action is the software; interfaces are its projections.

A meaningful capability should have one headless action core. Chat, GUI, CLI, API, desktop, mobile,
and future devices should call that same capability rather than reimplement it.

We call this:

> **One Core, Many Shadows.**

The action is the source. Each interface is a shadow cast for a particular person, agent, device,
and moment.

### 10. A Pod should do one clear thing and compose with others.

A Pod is a small executable interaction unit with a clear input, output, state, permission
boundary, and recovery path. A useful Pod does not attempt to become an entire application. Its
structured contract allows people and AI to discover, invoke, connect, and replace it.

### 11. Data and authority require explicit boundaries.

Every Pod should declare what data it needs, where that data may go, what it can change, and which
capabilities it can invoke. Prefer local over remote, read over write, narrow scope over broad
access, and temporary authority over permanent authority.

### 12. AI GUI should be open, portable, and inspectable.

People should be able to create, study, modify, share, and replace Pods. AI systems should be able
to discover and invoke them through stable contracts. AI GUI must not belong to one model,
operating system, vendor, or application store.

---

## What is a Pod?

A Pod is not a smaller traditional app.

It is not a widget, a browser extension, or a panel floating beside chat.

A Pod is the visible, executable form of an action. It combines:

- **Action** — the capability the system can execute.
- **Interface** — the smallest surface needed to understand, choose, and confirm.
- **Schema** — structured input, output, and effects that software can validate.
- **State** — the task's current version, progress, and result.
- **Permission** — the boundary around data and capabilities.
- **Recovery** — the path for failure, undo, restoration, and safe re-execution.

A crop selector can be a Pod. So can an annotation tool, a location confirmation, a date picker, a
QR replacement, or a document signature.

> **A Pod is not a miniature app. A Pod is an action made visible.**

---

## The relationship between AI GUI and PodApp

**AI GUI** is the interaction philosophy.

**The Action Core** is the capability itself.

**A Pod** is the smallest runnable AI GUI unit.

**A Shadow** is one presentation of that capability on a particular surface.

**PodApp** is an open reference implementation of these ideas.

The manifesto must remain larger than the product. The protocol must remain larger than the
platform. The ecosystem must remain larger than any one organization.

---

## What we reject

We reject adding AI merely to decorate existing products.

We reject replacing every professional interaction with a chat box.

We reject invisible automation with unlimited authority.

We reject treating the ability to act as permission to act.

We reject systems that cannot be understood, interrupted, audited, or recovered.

We reject rebuilding open software capabilities as closed, model-specific plugins.

We do not reject chat, agents, automation, or increasingly capable AI.

We reject **intelligence without boundaries**.

---

## What we call for

Let AI understand and reason.

Let actions execute.

Let interfaces clarify and confirm.

Let protocols connect.

Let people retain final authority.

We call on designers, developers, researchers, model builders, operating-system teams, and tool
makers to build interfaces in which humans and AI can see the same state, share the same action,
understand the same consequences, and recover from the same mistakes.

We do not ask for more interface.

We ask for more necessary interface.

---

## Declaration

The future will not be one endless chat, and it will not return to the menu-filled software of the
past.

It will combine the open possibility of AI with the explicit boundaries of interfaces.

AI explores. GUI narrows.

AI generates. GUI confirms.

AI proposes. People decide.

**The future is not No GUI.**

**It is Just Enough GUI.**

Let language carry expression.

Let models carry reasoning.

Let actions carry execution.

Let GUI carry certainty.

Let people carry the decision.

> **Give ambiguity to AI. Give boundaries to GUI. Keep authority with people.**

---

**AI GUI Manifesto · Version 0.1**

Initiated by [PodApp](https://podapp.net) · July 2026

This manifesto belongs to no single model, platform, or company. You are invited to sign,
translate, discuss, challenge, implement, and improve it.

The text of this manifesto is licensed under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).

[简体中文版](./MANIFESTO.zh-CN.md) · [Principles](./PRINCIPLES.md) ·
[Sign the manifesto](./SIGNATORIES.md)
