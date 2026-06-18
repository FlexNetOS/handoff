# repomix-rs Integration Research

## Overview
**Repository:** https://github.com/sopaco/repomix-rs  
**Purpose:** Repo context gatherer for LLM consumption  
**Relevance:** HIGH - directly relevant to handoff's repo map and context capsule needs

## What it Does
- Harvests codebase context in a format optimized for LLMs
- Handles large repositories better than our current map generation
- Creates context files that can be fed to LLMs

## Integration Approach
1. CLI tool - can be wrapped by `hf index` command
2. No API dependency - pure CLI integration  
3. Can replace/extend our current `.handoff/maps/repo-map.json`

## Advantages Over Current Implementation
- More mature codebase for context gathering
- Better handling of large repositories
- Pre-built solutions for common patterns

## Integration Points
- `hf index` command could call repomix-rs as one source
- Context capsule could include repomix-generated summary
- Maps directory could contain both generated and repomix outputs

## Recommendation: ADD
**Status:** ✅ Research complete - should be added as an optional enhancement  
**Priority:** Low (nice-to-have, not blocking)
