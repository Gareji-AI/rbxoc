#!/usr/bin/env node
// Compatibility entry point: deliberately shares the exact same safety policy.
import { runSweep } from "./live-sweep-runner.mjs";
process.exitCode = runSweep();
