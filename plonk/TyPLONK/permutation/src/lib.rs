use ark_bls12_381::Fr;
use ark_ff::{One, Zero};
use ark_poly::{EvaluationDomain, Evaluations, GeneralEvaluationDomain, Polynomial};
use kgz::{KzgCommitment, KzgScheme};
use std::{
    collections::{HashMap, HashSet},
    mem::swap,
};

pub mod proving;

#[derive(Hash, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Tag {
    pub i: usize,
    pub j: usize,
}
impl Tag {
    /// flatten tag to index
    fn to_index(&self, rows: &usize) -> usize {
        self.j + self.i * rows
    }
    fn from_index(index: &usize, rows: &usize) -> Self {
        let i = index / rows;
        let j = index % rows;
        Tag { i, j }
    }
}
#[derive(Default, Debug)]
pub struct PermutationBuilder<const C: usize> {
    /// stores the constraints(equal) between tags
    constrains: HashMap<Tag, Vec<Tag>>,
    /// the number of rows in the matrix(contraints number)
    rows: usize,
}

impl<const C: usize> PermutationBuilder<C> {
    pub fn add_row(&mut self) {
        self.rows += 1;
    }
    pub fn with_rows(rows: usize) -> Self {
        let mut new = Self::default();
        new.rows = rows;
        new
    }
    ///checks if a tag is valid
    fn check_tag(&self, tag: &Tag) -> bool {
        let Tag { i, j } = tag;
        i <= &C && j < &self.rows
    }
    pub fn add_constrain(&mut self, left: Tag, right: Tag) -> Result<(), ()> {
        if !(self.check_tag(&left) && self.check_tag(&right)) {
            Err(())
        } else {
            let cell = self.constrains.entry(left).or_insert(Vec::with_capacity(8));
            cell.push(right);
            Ok(())
        }
    }
    pub fn add_constrains(&mut self, constrains: Vec<(Tag, Tag)>) {
        for (left, right) in constrains {
            self.add_constrain(left, right).unwrap();
        }
    }

    /// designed to execute the permutation based on the constraints that have been added. It effectively ensures that the variables constrained to be equal are correctly permuted by swapping their positions in the permutation vector
    pub fn build(&mut self, size: usize) -> Permutation<C> {
        // C is 3 so actually `len` is len[w_a, w_b, w_c]*len[q_l, q_r, q_m, q_c, q_o]
        let len = size * C;
        //  indicating that initially, each element is in its original position.
        // The mapping vector helps in tracking the current state of the permutation
        // mapping a kind of reflection, mapping from tag position to new position mapping[tag]
        let mut mapping = (0..len).collect::<Vec<_>>();
        // the aux vector as a mapping where each position (index) represents a tag, and the value at that position represents the parent tag or the representative element of the set to which the tag belongs.
        let mut aux = (0..len).collect::<Vec<_>>();
        // used to keep track of the size of each set in the union-find structure.
        let mut sizes = std::iter::repeat(1).take(len).collect::<Vec<_>>();
        // take the value from self.constrains and then set it to empty
        let constrains = std::mem::take(&mut self.constrains);
        for (left, rights) in constrains.into_iter() {
            let mut left = left.to_index(&size);
            // union-find structure
            // Find: Determines the root of the set containing a given element. This is done by following parent pointers until a root is reached.
            // Union: Merges two sets. This is achieved by making the root of one set point to the root of the other.
            // utilizes a union-find data structure to manage the merging of tags that are constrained to be equal
            for right in rights {
                let mut right = right.to_index(&size);
                // check if already merged
                if aux[left] == aux[right] {
                    continue;
                }
                // Ensure the smaller set is merged into the larger set to keep the tree balanced.
                if sizes[aux[left]] < sizes[aux[right]] {
                    swap(&mut left, &mut right);
                }
                sizes[aux[left]] += sizes[aux[right]];
                // merge set and update `aux` and `mapping`
                // aux_left is root
                let mut next = right;
                let aux_left = aux[left];
                // This loop is ensuring that the newly merged set has consistent parent (or root) information across all its elements.
                /* The aux array is used to store the parent or representative of each element. Initially, each element is its own parent.
                   When two sets are merged, the parent of the right set is set to the parent of the left set.
                   When merging two sets, not only the direct elements but also all elements in the chain need to be updated to reflect the new parent/root.
                 */
                loop {
                    // This line sets the parent of next to aux_left, which is the parent of left.
                    aux[next] = aux_left;
                    next = mapping[next];
                    // When merging two sets, not only the direct elements but also all elements in the chain need to be updated to reflect the new parent/root.
                    if aux[next] == aux_left {
                        break;
                    }
                }
                mapping.swap(left, right);
            }
        }
        Permutation { perm: mapping }
    }
}
#[derive(Debug)]
pub struct Permutation<const C: usize> {
    /// A vector that holds the permutation. Each element in the vector indicates the new position of the corresponding tag after applying the permutation
    /// 
    /// for example `let perm = vec![3, 1, 2, 0, 4, 5, 6, 7, 8, 9, 10, 11];`
    /// The element originally at index 0 is now at index 3.
    /// The element originally at index 3 is now at index 0.
    /// The elements at indices 1, 2, 4, 5, 6, 7, 8, 9, 10, and 11 remain in their original positions.
    perm: Vec<usize>,
}

impl<const C: usize> Permutation<C> {
    pub fn compile(self) -> CompiledPermutation<C> {
        // permu now is permued mapping
        assert_eq!(self.perm.len() % C, 0);
        let rows = self.perm.len() / C;
        let cols = self.perm.chunks(rows);
        let cosets = Self::cosets(rows);
        let domain = <GeneralEvaluationDomain<Fr>>::new(rows).unwrap();
        let roots = domain.elements().collect::<Vec<_>>();
        let perm = cols.enumerate().map(|(i, col)| {
            // generate a collection of (tag, value) pairs
            // index is permued value
            let coefficients = col
                .iter()
                .enumerate()
                .map(|(j, index)| {
                    let tag = Tag::from_index(index, &rows);
                    // value is \sigma_
                    let value = cosets[tag.i] * roots[tag.j];
                    //  cosets is [c_0, c_1, c_2], this calculate c*w_i, tag is id_
                    let tag = cosets[i] * roots[j];
                    (tag, value)
                })
                .collect();
            coefficients
            //let poly = DensePolynomial::from_coefficients_vec(coefficients);
            //poly
        });
        let mut cols: [Vec<(Fr, Fr)>; C] = [0_u8; C].map(|_| Default::default());
        for (i, col) in perm.enumerate() {
            cols[i] = col;
        }
        CompiledPermutation { cols, cosets, rows }
    }
    pub fn print(&self) {
        println!("len: {}", self.perm.len());
        let rows = self.perm.len() / C;
        let perm = &self.perm;
        for j in 0..rows {
            let mut row = vec![];
            for i in 0..C {
                row.push(perm[j + i * rows]);
            }
            println!("{:?}", row);
        }
    }
    fn cosets(gates: usize) -> [Fr; C] {
        let domain = <GeneralEvaluationDomain<Fr>>::new(gates).unwrap();
        let mut cosets = [Fr::zero(); C];

        let mut k = Fr::one();
        for coset in cosets.iter_mut() {
            // check if k is a root of the vanishing polynomial of the domain.
            while domain.evaluate_vanishing_polynomial(k).is_zero() {
                k += Fr::from(1);
            }
            *coset = k;
            k += Fr::from(1);
        }
        cosets
    }
}
#[derive(Debug)]
pub struct CompiledPermutation<const C: usize> {
    //cols: Vec<Vec<(Fr, Fr)>>,
    pub cols: [Vec<(Fr, Fr)>; C],
    pub cosets: [Fr; C],
    rows: usize,
}

impl<const C: usize> CompiledPermutation<C> {
    pub fn sigma_evals(&self, point: &Fr, domain: GeneralEvaluationDomain<Fr>) -> [Fr; C] {
        self.cols
            .iter()
            .map(|col| {
                let evals = col.iter().map(|cell| cell.1).collect();
                let eval = <Evaluations<Fr>>::from_vec_and_domain(evals, domain);
                let poly = eval.interpolate();
                poly.evaluate(point)
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
    pub fn sigma_commitments(
        &self,
        scheme: &KzgScheme,
        domain: GeneralEvaluationDomain<Fr>,
    ) -> [KzgCommitment; C] {
        self.cols
            .iter()
            .map(|col| {
                let evals = col.iter().map(|cell| cell.1).collect();
                let eval = <Evaluations<Fr>>::from_vec_and_domain(evals, domain);
                let poly = eval.interpolate();
                scheme.commit(&poly)
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
}

#[allow(dead_code)]
fn print_matrix(matrix: &[usize], cols: usize) {
    println!();
    for row in matrix.chunks(cols) {
        println!("{:?}", row);
    }
}

#[allow(dead_code)]
fn print_cycle(elems: &[usize]) {
    let mut seen = HashSet::new();
    for elem in elems {
        if seen.contains(elem) {
            continue;
        } else {
            seen.insert(*elem);
            let mut cycle = vec![*elem];
            let mut next = elems[*elem];
            loop {
                if seen.contains(&next) {
                    break;
                }
                seen.insert(next);
                cycle.push(next);
                next = elems[next];
            }
            println!("{:?}", cycle);
        }
    }
}
