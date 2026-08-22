---
title: devtools · landing screenshots
description: Playwright visual-regression baselines for the landing page, one per breakpoint.
---

**Source:** `docs/tests/landing.spec.ts`

**Regenerate:** `pnpm run test:update` (from `docs/`) — runs the suite inside the pinned
Playwright container (ADR 0031) and copies the resulting baselines here.

One screenshot per breakpoint `landing.spec.ts` tests against.

![landing-big-monitor-linux](../../../../assets/devtools/landing/landing-big-monitor-linux.png)
![landing-desktop-linux](../../../../assets/devtools/landing/landing-desktop-linux.png)
![landing-laptop-linux](../../../../assets/devtools/landing/landing-laptop-linux.png)
![landing-tablet-landscape-linux](../../../../assets/devtools/landing/landing-tablet-landscape-linux.png)
![landing-tablet-linux](../../../../assets/devtools/landing/landing-tablet-linux.png)
![landing-mobile-landscape-linux](../../../../assets/devtools/landing/landing-mobile-landscape-linux.png)
![landing-mobile-linux](../../../../assets/devtools/landing/landing-mobile-linux.png)
