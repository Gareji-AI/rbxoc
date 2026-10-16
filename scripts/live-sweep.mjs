#!/usr/bin/env node
// Read-only unless RBXOC_SWEEP_MUTATE=1; configuration is validated before use.
import { runSweep } from "./live-sweep-runner.mjs";
process.exitCode = runSweep();
