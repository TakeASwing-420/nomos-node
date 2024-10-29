use crate::{routing::EncryptedRoutingInformation, Error};
use sphinx_packet::{
    constants::NODE_ADDRESS_LENGTH,
    header::{
        keys::RoutingKeys,
        routing::{FINAL_HOP, FORWARD_HOP},
    },
    payload::Payload,
};

/// A packet that contains a header and a payload.
/// The header and payload are encrypted for the selected recipients.
/// This packet can be serialized and sent over the network.
#[derive(Debug)]
pub struct Packet {
    header: Header,
    // This crate doesn't limit the payload size.
    // Users must choose the appropriate constant size and implement padding.
    payload: Vec<u8>,
}

/// The packet header
#[derive(Debug)]
struct Header {
    /// The ephemeral public key for a recipient to derive the shared secret
    /// which can be used to decrypt the header and payload.
    ephemeral_public_key: x25519_dalek::PublicKey,
    encrypted_routing_info: EncryptedRoutingInformation,
}

impl Packet {
    pub fn build(
        recipient_pubkeys: &[x25519_dalek::PublicKey],
        max_layers: usize,
        payload: &[u8],
        payload_size: usize,
    ) -> Result<Self, Error> {
        // Derive `[sphinx_packet::header::keys::KeyMaterial]` for all recipients.
        let ephemeral_privkey = x25519_dalek::StaticSecret::random();
        let key_material = Self::derive_key_material(recipient_pubkeys, &ephemeral_privkey);

        // Build the encrypted routing information.
        let encrypted_routing_info =
            EncryptedRoutingInformation::new(&key_material.routing_keys, max_layers)?;

        // Encrypt the payload for all recipients.
        let payload_keys = key_material
            .routing_keys
            .iter()
            .map(|key_material| key_material.payload_key)
            .collect::<Vec<_>>();
        let payload = sphinx_packet::payload::Payload::encapsulate_message(
            payload,
            &payload_keys,
            payload_size,
        )?;

        Ok(Packet {
            header: Header {
                ephemeral_public_key: x25519_dalek::PublicKey::from(&ephemeral_privkey),
                encrypted_routing_info,
            },
            payload: payload.into_bytes(),
        })
    }

    pub(crate) fn derive_key_material(
        recipient_pubkeys: &[x25519_dalek::PublicKey],
        ephemeral_privkey: &x25519_dalek::StaticSecret,
    ) -> sphinx_packet::header::keys::KeyMaterial {
        // NodeAddress is needed to build [`sphinx_packet::route::Node`]s
        // required by [`sphinx_packet::header::keys::KeyMaterial::derive`],
        // but it's not actually used inside. So, we can use a dummy address.
        let dummy_node_address =
            sphinx_packet::route::NodeAddressBytes::from_bytes([0u8; NODE_ADDRESS_LENGTH]);
        let route = recipient_pubkeys
            .iter()
            .map(|pubkey| sphinx_packet::route::Node {
                address: dummy_node_address,
                pub_key: *pubkey,
            })
            .collect::<Vec<_>>();
        sphinx_packet::header::keys::KeyMaterial::derive(&route, ephemeral_privkey)
    }

    pub fn unpack(
        &self,
        private_key: &x25519_dalek::StaticSecret,
        max_layers: usize,
    ) -> Result<UnpackedPacket, Error> {
        // Derive the routing keys for the recipient
        let routing_keys = sphinx_packet::header::SphinxHeader::compute_routing_keys(
            &self.header.ephemeral_public_key,
            private_key,
        );

        // Decrypt one layer of encryption on the payload
        let payload = sphinx_packet::payload::Payload::from_bytes(&self.payload)?;
        let payload = payload.unwrap(&routing_keys.payload_key)?;

        // Unpack the routing information
        let (routing_info, next_encrypted_routing_info) = self
            .header
            .encrypted_routing_info
            .unpack(&routing_keys, max_layers)?;
        match routing_info.flag {
            FORWARD_HOP => Ok(UnpackedPacket::ToForward(self.build_next_packet(
                &routing_keys,
                next_encrypted_routing_info,
                payload,
            ))),
            FINAL_HOP => Ok(UnpackedPacket::FullyUnpacked(payload.recover_plaintext()?)),
            _ => return Err(Error::InvalidRoutingFlag(routing_info.flag)),
        }
    }

    fn build_next_packet(
        &self,
        routing_keys: &RoutingKeys,
        next_encrypted_routing_info: EncryptedRoutingInformation,
        payload: Payload,
    ) -> Packet {
        // Derive the new ephemeral public key for the next recipient
        let next_ephemeral_pubkey = Self::derive_next_ephemeral_public_key(
            &self.header.ephemeral_public_key,
            &routing_keys.blinding_factor,
        );
        Packet {
            header: Header {
                ephemeral_public_key: next_ephemeral_pubkey,
                encrypted_routing_info: next_encrypted_routing_info,
            },
            payload: payload.into_bytes(),
        }
    }

    /// Derive the next ephemeral public key for the next recipient.
    //
    // This is a copy of `blind_the_shared_secret` from https://github.com/nymtech/sphinx/blob/344b902df340e0d5af69c5147b05f76f324b8cef/src/header/mod.rs#L234.
    // with renaming the function name and the arguments
    // because the original function is not exposed to the public.
    // This logic is tightly coupled with the [`sphinx_packet::header::keys::KeyMaterial::derive`].
    fn derive_next_ephemeral_public_key(
        cur_ephemeral_pubkey: &x25519_dalek::PublicKey,
        blinding_factor: &x25519_dalek::StaticSecret,
    ) -> x25519_dalek::PublicKey {
        let new_shared_secret = blinding_factor.diffie_hellman(cur_ephemeral_pubkey);
        x25519_dalek::PublicKey::from(new_shared_secret.to_bytes())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.header.ephemeral_public_key.to_bytes());
        bytes.extend_from_slice(&self.header.encrypted_routing_info.to_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn from_bytes(data: &[u8], max_layers: usize) -> Result<Self, Error> {
        let mut i = 0;
        let public_key_bytes: [u8; 32] = data[i..i + 32].try_into().unwrap();
        let ephemeral_public_key = x25519_dalek::PublicKey::from(public_key_bytes);
        i += 32;

        let encrypted_routing_info_size = EncryptedRoutingInformation::size(max_layers);
        let encrypted_routing_info = EncryptedRoutingInformation::from_bytes(
            &data[i..i + encrypted_routing_info_size],
            max_layers,
        )?;
        i += encrypted_routing_info_size;

        let payload = data[i..].to_vec();

        Ok(Packet {
            header: Header {
                ephemeral_public_key,
                encrypted_routing_info,
            },
            payload,
        })
    }
}

pub enum UnpackedPacket {
    ToForward(Packet),
    FullyUnpacked(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpack() {
        // Prepare keys of two recipients
        let recipient_privkeys = (0..3)
            .map(|_| x25519_dalek::StaticSecret::random())
            .collect::<Vec<_>>();
        let recipient_pubkeys = recipient_privkeys
            .iter()
            .map(x25519_dalek::PublicKey::from)
            .collect::<Vec<_>>();

        // Build a packet
        let max_layers = 5;
        let payload = [10u8; 512];
        let packet = Packet::build(&recipient_pubkeys, max_layers, &payload, 1024).unwrap();

        // The 1st recipient unpacks the packet
        let packet = match packet.unpack(&recipient_privkeys[0], max_layers).unwrap() {
            UnpackedPacket::ToForward(packet) => packet,
            UnpackedPacket::FullyUnpacked(_) => {
                panic!("The unpacked packet should be the ToFoward type");
            }
        };
        // The 2nd recipient unpacks the packet
        let packet = match packet.unpack(&recipient_privkeys[1], max_layers).unwrap() {
            UnpackedPacket::ToForward(packet) => packet,
            UnpackedPacket::FullyUnpacked(_) => {
                panic!("The unpacked packet should be the ToFoward type");
            }
        };
        // The last recipient unpacks the packet
        match packet.unpack(&recipient_privkeys[2], max_layers).unwrap() {
            UnpackedPacket::ToForward(_) => {
                panic!("The unpacked packet should be the FullyUnpacked type");
            }
            UnpackedPacket::FullyUnpacked(unpacked_payload) => {
                // Check if the payload has been decrypted correctly
                assert_eq!(unpacked_payload, payload);
            }
        }
    }

    #[test]
    fn unpack_with_wrong_keys() {
        // Build a packet with two public keys
        let max_layers = 5;
        let payload = [10u8; 512];
        let packet = Packet::build(
            &(0..2)
                .map(|_| x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::random()))
                .collect::<Vec<_>>(),
            max_layers,
            &payload,
            1024,
        )
        .unwrap();

        assert!(packet
            .unpack(&x25519_dalek::StaticSecret::random(), max_layers)
            .is_err());
    }

    #[test]
    fn consistent_size_after_unpack() {
        // Prepare keys of two recipients
        let recipient_privkeys = (0..2)
            .map(|_| x25519_dalek::StaticSecret::random())
            .collect::<Vec<_>>();
        let recipient_pubkeys = recipient_privkeys
            .iter()
            .map(x25519_dalek::PublicKey::from)
            .collect::<Vec<_>>();

        // Build a packet
        let max_layers = 5;
        let payload = [10u8; 512];
        let packet = Packet::build(&recipient_pubkeys, max_layers, &payload, 1024).unwrap();

        // Calculate the expected packet size
        let pubkey_size = 32;
        let payload_size = 1024;
        let packet_size =
            pubkey_size + EncryptedRoutingInformation::size(max_layers) + payload_size;

        // The serialized packet size must be the same as the expected size.
        assert_eq!(packet.to_bytes().len(), packet_size);

        // The unpacked packet size must be the same as the original packet size.
        match packet.unpack(&recipient_privkeys[0], max_layers).unwrap() {
            UnpackedPacket::ToForward(packet) => {
                assert_eq!(packet.to_bytes().len(), packet_size);
            }
            UnpackedPacket::FullyUnpacked(_) => {
                panic!("The unpacked packet should be the ToFoward type");
            }
        }
    }

    #[test]
    fn consistent_size_with_any_num_layers() {
        let max_layers = 5;
        let payload = [10u8; 512];

        // Build a packet with 2 recipients
        let recipient_pubkeys = (0..2)
            .map(|_| x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::random()))
            .collect::<Vec<_>>();
        let packet1 = Packet::build(&recipient_pubkeys, max_layers, &payload, 1024).unwrap();

        // Build a packet with 3 recipients
        let recipient_pubkeys = (0..3)
            .map(|_| x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::random()))
            .collect::<Vec<_>>();
        let packet2 = Packet::build(&recipient_pubkeys, max_layers, &payload, 1024).unwrap();

        assert_eq!(packet1.to_bytes().len(), packet2.to_bytes().len());
    }
}
