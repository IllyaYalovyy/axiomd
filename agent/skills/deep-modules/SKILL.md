---
name: deep-modules
description: Enforced code-structure standard — deep modules with simple APIs. Load before designing any interface, adding any public item, or reviewing any diff.
---

# Deep modules, simple APIs

Owner mandate (2026-08-02), injected into every task: **every module hides
significant functionality behind a small interface.** Drift is not
allowed. Each commit leaves the codebase structurally tighter than it
found it.

## The standard

- A module (crate, `pub mod`, type with methods) earns its existence by
  the complexity it HIDES. Interface small, implementation substantial.
- **Shallow modules are design defects**: a type or function whose
  interface is as complex as its implementation, pass-through layers that
  re-export or forward with no added invariant, "manager"/"util"/"helper"
  grab-bags, traits with one impl and no boundary purpose.
- Complex interfaces are defects even on deep modules: long parameter
  lists, boolean/flag soup, config structs that expose internals, leaky
  types (a comrak/webkit/sourceview type in a public signature outside its
  home crate), APIs that require call-ordering knowledge to use safely.
- Prefer: fewer, larger, deeper modules; define errors out of existence
  (make illegal states unrepresentable, choose defaults over knobs);
  general-purpose interfaces slightly deeper than today's one caller
  needs — but never speculative plugin points nobody uses.

## Mechanical checks before completion

1. List every public item your diff adds or widens (`pub fn`, `pub
   struct` fields, trait methods, feature flags). For each: what does it
   hide? If the honest answer is "nothing — it forwards", inline it.
2. Count callers: a new abstraction with exactly one caller and no hidden
   invariant is premature — inline it.
3. Ratchet direction: the task report states whether the public API
   surface of touched crates grew, held, or shrank — and why growth was
   necessary. "It grew because it was convenient" fails review.
4. Opportunistic tightening: if the code you touched has a shallow module
   or leaky signature IN your change's blast radius, deepen it as part of
   the task. Outside the blast radius: file it in the report, don't churn.

## Anti-capitulation

Do not satisfy this skill by hiding code in private functions while the
public surface stays wide, by merging unrelated modules to game the count,
or by deleting documentation. The test is always: can a caller use this
module correctly knowing only its interface?
