---
title: Nostr Wallet Architecture
slug: nostr-wallet-architecture
topic: nostr-wallet
summary: The project is built in Rust on the nostr-sdk crate, pinned to version 0.31
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-06
updated: 2026-06-06
verified: 2026-06-06
compiled-from: conversation
sources:
  - session:8240399a-f332-4082-8a4f-6c60dd67f9a6
---

# Nostr Wallet Architecture

## Technology Stack

The project is built in Rust on the nostr-sdk crate, pinned to version 0.31. It uses tokio for the async runtime and serde for serialization. <!-- [^82403-1] -->

## Architecture

nostr-sdk is wrapped in a NostrClient type with a binary + library split so functionality can be reused and expanded. <!-- [^82403-2] -->

## Application Identity

The application is a NIP-60 Cashu wallet. It supports NIP-61 nutzaps and can redeem a user's incoming npub.cash zaps; this redemption is configurable. <!-- [^82403-3] -->

## CLI and Onboarding

The wallet is a CLI invoked as ./wallet. On first run it offers onboarding: the user either registers (generating a new nsec) or logs in with an existing nsec. On registration, existing users fetch their relay list; new users start with an empty relay list. After the user is authenticated, the wallet checks whether they already have a NIP-60 wallet created. <!-- [^82403-4] -->

## Core Features

The wallet lets the user send payments, check their balance, and manage their tokens. <!-- [^82403-5] -->

## Relay and Mint Discovery

purplepag.es is used to fetch the user's kind:10002 (NIP-65) relay list. Mint discovery uses NIP-87 (kind:38172 events). <!-- [^82403-6] -->
