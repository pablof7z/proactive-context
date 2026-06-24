---
title: Fabric Domain Model
slug: fabric-domain-model
topic: fabric-architecture
summary: The domain verbs are organized into two concern-planes â Project-State (open_project, roster, presence, status, project_meta, list_projects) and Communication
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-09
updated: 2026-06-09
verified: 2026-06-09
compiled-from: conversation
sources:
  - session:23f399e7-912c-4c7b-b960-4a1044e144ca
---

# Fabric Domain Model

## Domain Verbs & Concern Planes

The domain verbs are organized into two concern-planes — Project-State (open_project, roster, presence, status, project_meta, list_projects) and Communications (send, inbox, threads, thread_meta) — plus one ACL (the is_member predicate both planes consult). <!-- [^23f39-2] -->

NIP-29 group management is an access-control/addressing concern orthogonal to event wire-shaping, and should not be a property of the kind1 event codec but rather a property of a nostr transport/ACL strategy. <!-- [^23f39-3] -->

NIP-29 enforces membership server-side, MLS enforces it cryptographically, and kind1 enforces it client-side — which is why the is_member gate must live in the domain layer where it can never be skipped. <!-- [^23f39-4] -->

The is_member ACL gate is consulted twice over the same store rows — once as a write-side admission predicate during materialization, once as a read-side query — never on the wire. <!-- [^23f39-5] -->

Project metadata provenance varies per fabric: nip29 uses relay-authored kind:39000 (canonical and shared), mls uses member-authored group-context (cryptographically scoped), kind1 has no native carrier (description is Option/local, list is derived from observed tags and local dirs). <!-- [^23f39-6] -->

Threads are a store noun (not a wire concept) that the materializer derives from message relationships. <!-- [^23f39-7] -->

## Read Model

All data is read from a unified local store; how that data is hydrated is completely invisible to readers — the provider is a write-side materializer, not a read server. <!-- [^23f39-8] -->

The single-writer daemon owning state.db is the direct fix for multi-writer corruption. <!-- [^23f39-9] -->

## FabricProvider Capabilities

The FabricProvider bundles four single-responsibility capabilities: Lifecycle reactor, Materializer (composes Wire-codec + Delivery, owns only admit + derive + upsert), Wire codec (pure DomainEvent ⇄ envelope), and Delivery (publish + subscribe-for-scope). <!-- [^23f39-10] -->

The current Codec trait is coupled to nostr_sdk types (EventBuilder, Event, Filter), so it can only swap NIPs, not transports; the filters verb bakes in relay REQ semantics and cannot survive a real transport swap. <!-- [^23f39-11] -->

A new codec must round-trip all five DomainEvent variants (Profile, Presence, Activity, Status, Mention); its filters must fetch everything its decode accepts, and nothing in domain, runtime, or state changes if that contract holds. <!-- [^23f39-12] -->

## Messaging Addressing

The message row must carry its own return envelope (from_session / author_session), not just the author pubkey, because sibling sessions share a pubkey and pubkey alone cannot address a reply to the right session. <!-- [^23f39-13] -->

Injected mentions carry the sender's session as a reply address, formatted so the recipient knows exactly which session to reply to. <!-- [^23f39-14] -->
