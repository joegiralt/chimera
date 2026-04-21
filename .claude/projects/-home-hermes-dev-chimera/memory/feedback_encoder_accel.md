---
name: Encoder acceleration curve
description: User-approved encoder acceleration curve for PreenFM3 rotary encoders — per-click values and design constraints
type: feedback
---

Encoder acceleration curve: 1, 2, 4, 5, 10, 10, 10... per consecutive fast click. Burst resets to 0 when no edges arrive in a frame.

**Why:** Single clicks must be precise (±1), fast spins must cover full range quickly. Previous attempts with ISR-side acceleration caused ghost movement after stopping due to mechanical bounce amplification.

**How to apply:** Acceleration must live in the main thread (snapshot), not the ISR. ISR only produces raw ±1 edges with debounce (2-tick dead zone). The burst counter resets immediately when no edges arrive, so values stop dead when the encoder stops. N24 quadrature table (not N12) — gives 1 edge per detent click.
