use crate::message::{compute_hash, Hash};
use serde::{Deserialize, Serialize};

/// A simple Merkle tree implementation for transaction roots
#[derive(Debug, Clone)]
pub struct MerkleTree {
    /// Leaf hashes
    leaves: Vec<Hash>,
    /// Computed root
    root: Hash,
    /// All nodes (for proof generation)
    nodes: Vec<Vec<Hash>>,
}

impl MerkleTree {
    /// Create a new Merkle tree from leaf hashes
    pub fn new(leaves: &[Hash]) -> Self {
        if leaves.is_empty() {
            return Self {
                leaves: Vec::new(),
                root: [0u8; 32],
                nodes: Vec::new(),
            };
        }

        let mut tree = Self {
            leaves: leaves.to_vec(),
            root: [0u8; 32],
            nodes: Vec::new(),
        };
        tree.build();
        tree
    }

    /// Create a Merkle tree from transaction hashes
    pub fn from_transactions<T: AsRef<[u8]>>(items: &[T]) -> Self {
        let leaves: Vec<Hash> = items.iter().map(|item| compute_hash(item.as_ref())).collect();
        Self::new(&leaves)
    }

    /// Build the tree from leaves
    fn build(&mut self) {
        if self.leaves.is_empty() {
            return;
        }

        let mut current_level = self.leaves.clone();
        self.nodes.push(current_level.clone());

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(2) {
                let hash = if chunk.len() == 2 {
                    hash_pair(&chunk[0], &chunk[1])
                } else {
                    // Odd number of nodes: duplicate the last one
                    hash_pair(&chunk[0], &chunk[0])
                };
                next_level.push(hash);
            }

            self.nodes.push(next_level.clone());
            current_level = next_level;
        }

        self.root = current_level[0];
    }

    /// Get the Merkle root
    pub fn root(&self) -> Hash {
        self.root
    }

    /// Get the number of leaves
    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    /// Check if tree is empty
    pub fn is_empty(&self) -> bool {
        self.leaves.is_empty()
    }

    /// Generate a proof for a leaf at the given index
    pub fn proof(&self, index: usize) -> Option<MerkleProof> {
        if index >= self.leaves.len() {
            return None;
        }

        let mut proof_hashes = Vec::new();
        let mut proof_indices = Vec::new();
        let mut current_index = index;

        for level in 0..self.nodes.len() - 1 {
            let level_len = self.nodes[level].len();
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };

            if sibling_index < level_len {
                proof_hashes.push(self.nodes[level][sibling_index]);
                proof_indices.push(current_index % 2 == 1); // true if sibling is on the left
            } else {
                // Odd number case: sibling is the same as current
                proof_hashes.push(self.nodes[level][current_index]);
                proof_indices.push(false);
            }

            current_index /= 2;
        }

        Some(MerkleProof {
            leaf: self.leaves[index],
            leaf_index: index,
            proof_hashes,
            proof_indices,
            root: self.root,
        })
    }
}

/// A Merkle proof for a single leaf
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// The leaf hash being proven
    pub leaf: Hash,
    /// Index of the leaf
    pub leaf_index: usize,
    /// Hashes along the proof path
    pub proof_hashes: Vec<Hash>,
    /// Direction indicators (true = sibling is on the left)
    pub proof_indices: Vec<bool>,
    /// Expected root
    pub root: Hash,
}

impl MerkleProof {
    /// Verify this proof
    pub fn verify(&self) -> bool {
        Self::verify_proof(&self.leaf, &self.proof_hashes, &self.proof_indices, &self.root)
    }

    /// Verify a proof against a root
    pub fn verify_proof(
        leaf: &Hash,
        proof_hashes: &[Hash],
        proof_indices: &[bool],
        root: &Hash,
    ) -> bool {
        if proof_hashes.len() != proof_indices.len() {
            return false;
        }

        let mut current = *leaf;

        for (hash, is_left) in proof_hashes.iter().zip(proof_indices.iter()) {
            current = if *is_left {
                hash_pair(hash, &current)
            } else {
                hash_pair(&current, hash)
            };
        }

        current == *root
    }
}

/// Hash two nodes together
fn hash_pair(left: &Hash, right: &Hash) -> Hash {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(left);
    data.extend_from_slice(right);
    compute_hash(&data)
}

/// Compute the Merkle root of a list of transaction hashes
pub fn compute_merkle_root(hashes: &[Hash]) -> Hash {
    MerkleTree::new(hashes).root()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::new(&[]);
        assert_eq!(tree.root(), [0u8; 32]);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_single_leaf() {
        let leaf = compute_hash(b"hello");
        let tree = MerkleTree::new(&[leaf]);
        assert_eq!(tree.root(), leaf);
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_two_leaves() {
        let leaf1 = compute_hash(b"hello");
        let leaf2 = compute_hash(b"world");
        let tree = MerkleTree::new(&[leaf1, leaf2]);

        let expected_root = hash_pair(&leaf1, &leaf2);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_four_leaves() {
        let leaves: Vec<Hash> = (0..4).map(|i| compute_hash(&[i as u8])).collect();
        let tree = MerkleTree::new(&leaves);

        // Verify structure
        let h01 = hash_pair(&leaves[0], &leaves[1]);
        let h23 = hash_pair(&leaves[2], &leaves[3]);
        let expected_root = hash_pair(&h01, &h23);

        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn test_proof_generation_and_verification() {
        let leaves: Vec<Hash> = (0..4).map(|i| compute_hash(&[i as u8])).collect();
        let tree = MerkleTree::new(&leaves);

        for i in 0..4 {
            let proof = tree.proof(i).unwrap();
            assert!(proof.verify(), "Proof for leaf {} should be valid", i);
        }
    }

    #[test]
    fn test_proof_invalid_index() {
        let leaves: Vec<Hash> = (0..4).map(|i| compute_hash(&[i as u8])).collect();
        let tree = MerkleTree::new(&leaves);
        assert!(tree.proof(5).is_none());
    }

    #[test]
    fn test_odd_number_of_leaves() {
        let leaves: Vec<Hash> = (0..5).map(|i| compute_hash(&[i as u8])).collect();
        let tree = MerkleTree::new(&leaves);

        // All proofs should still verify
        for i in 0..5 {
            let proof = tree.proof(i).unwrap();
            assert!(proof.verify(), "Proof for leaf {} should be valid", i);
        }
    }

    #[test]
    fn test_modified_proof_fails() {
        let leaves: Vec<Hash> = (0..4).map(|i| compute_hash(&[i as u8])).collect();
        let tree = MerkleTree::new(&leaves);

        let mut proof = tree.proof(0).unwrap();
        proof.proof_hashes[0][0] ^= 0xff; // Corrupt the proof

        assert!(!proof.verify());
    }

    #[test]
    fn test_from_transactions() {
        let txs: Vec<Vec<u8>> = vec![b"tx1".to_vec(), b"tx2".to_vec(), b"tx3".to_vec()];
        let tree = MerkleTree::from_transactions(&txs);
        assert_eq!(tree.len(), 3);
        assert_ne!(tree.root(), [0u8; 32]);
    }
}
