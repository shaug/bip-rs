# Bittorrent Infrastructure Project
[![Build Status](https://travis-ci.org/GGist/bip-rs.svg)](https://travis-ci.org/GGist/bip-rs) [![Build status](https://ci.appveyor.com/api/projects/status/muiqrh76k5hoir0s/branch/master?svg=true)](https://ci.appveyor.com/project/GGist/bip-rs/branch/master)

A collection of crates for building applications using bittorrent technologies.

## Bencode (bip_bencode) - [![Docs](https://img.shields.io/badge/docs-up--to--date-blue.svg)](http://ggist.github.io/bip-rs/bip_bencode/index.html) [![Crate](http://meritbadge.herokuapp.com/bip_bencode)](https://crates.io/crates/bip_bencode)

**About**: Bencode is the binary encoding used throughout bittorrent technologies from metainfo files to DHT messages. Bencode types include integers, byte arrays, lists, and dictionarys, of which the last two can hold any bencode type (they could be recursively constructed).

## Handshake (bip_handshake) - [![Docs](https://img.shields.io/badge/docs-up--to--date-blue.svg)](http://ggist.github.io/bip-rs/bip_handshake/index.html) [![Crate](http://meritbadge.herokuapp.com/bip_handshake)](https://crates.io/crates/bip_handshake)

**About**: Handshaking is the process of connecting to a peer and exchanging information related to how a peer will be communicating with you and vice versa. In our case, there are many bittorrent technologies that could generally be considered peer discovery mechanisms (local peer discovery, dht, trackers, peer exchange) where once a peer is discovered, a client may want to immediately attempt to establish a connection via a handshake. This module provides a trait for custom handshake implementations, as well as the standard bittorrent handshake, so that clients can specify a handshaking mechanism for peer discovery services to forward contact information along to.

## Mainline DHT (bip_dht) - [![Docs](https://img.shields.io/badge/docs-up--to--date-blue.svg)](http://ggist.github.io/bip-rs/bip_dht/index.html) [![Crate](http://meritbadge.herokuapp.com/bip_dht)](https://crates.io/crates/bip_dht)

**About**: The Mainline DHT is used by bittorrent to distribute contact information for peers interested in specified files. More generally, any application can use the Mainline DHT to discover peers in a distributed and decentralized fashion. You can take advantage of the DHT as long as your application has a way of exposing interest in other peers via a SHA-1 hash (20 byte value).

**Example**: For example, a decentralized chat application could ask the user for a room name to join, you could then hash that room name and search for other peers using that application who are also interested in joining the same room.