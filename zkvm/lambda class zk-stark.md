
整个内容理论参考自 [lambda class starks-protocol](https://lambdaclass.github.io/lambdaworks/starks/protocol.html?highlight=grinding#starks-protocol)

## FRI

之所以要搞Merkel树是一个有限degree的函数证明，是防止prover给一个超级巨大的merkle树根，因为verifier会挑一些验证点来验证merkle树确实来自对应的多项式生成，如果prover生成的树时采用欺骗的手法，直接构造一个超级巨大接近有限域极限的merkle树，那么verifier去做challenge的时候就不太容易验证prover提供树确实来自多项式的树（prover完全可以用超级多的假值提前构造merkle树而verifier时无法发现的）

## Prover

### 数据预处理

在正式进入证明流程之前要去生成一些必要数据，方便后续使用。主要有两个执行代码

```rust
let air = A::new(main_trace.n_rows(), pub_inputs, proof_options);
let domain = Domain::new(&air);
```

`A::new()`是一个接口方法，不同的trace或者说成是表（比如fibonacci和Cairo是两个不同的东西）就需要有不同的约束设置，因此他们的约束初始化工作会有所不同。我们以`simple_fibonacci`为例，看一下他的AIR在new的时候是如何做的。

```rust
fn new(
	trace_length: usize,
	pub_inputs: &Self::PublicInputs,
	proof_options: &ProofOptions,
) -> Self {
	let constraints: Vec<Box<dyn TransitionConstraint<F, F>>> =
		vec![Box::new(FibConstraint::new())];

	let context = AirContext {
		proof_options: proof_options.clone(),
		trace_columns: 1,
		transition_exemptions: vec![2],
		transition_offsets: vec![0, 1, 2],
		num_transition_constraints: constraints.len(),
	};

	Self {
		pub_inputs: pub_inputs.clone(),
		context,
		trace_length,
		constraints,
	}
}
```

其中context表示将来在实际执行transition constraint的时候其上下文应该什么样子的。对于`simple_fibonacci`而言，其形式如下图所示

![Pasted image 20240906200819](https://github.com/user-attachments/assets/81780502-f7ba-4738-8d5d-0d68e9948151)


表的前两个数据是boundary constraint，所以要做`transition_exemption`。因为transition constraint需要3个数据才能表示，所以`transition_offsets`是3。

进入到`Domain::new`看一下，从其返回值来一个一个说明它是在做什么

```rust
pub struct Domain<F: IsFFTField> {
    pub(crate) root_order: u32,
    pub(crate) lde_roots_of_unity_coset: Vec<FieldElement<F>>,
    pub(crate) trace_primitive_root: FieldElement<F>,
    pub(crate) trace_roots_of_unity: Vec<FieldElement<F>>,
    pub(crate) coset_offset: FieldElement<F>,
    pub(crate) blowup_factor: usize,
    pub(crate) interpolation_domain_size: usize,
}
```

`root_order`实际是main trace polynomial order（和length不完全一致，有的时候为了一些目的会在trace后面补一些`0`来保证不同trace之间长度一致，所以不能单纯看length）。

整体看一下`Domain::new`的代码

```rust
pub struct Domain<F: IsFFTField> {
    pub(crate) root_order: u32,
    pub(crate) lde_roots_of_unity_coset: Vec<FieldElement<F>>,
    pub(crate) trace_primitive_root: FieldElement<F>,
    pub(crate) trace_roots_of_unity: Vec<FieldElement<F>>,
    pub(crate) coset_offset: FieldElement<F>,
    pub(crate) blowup_factor: usize,
    pub(crate) interpolation_domain_size: usize,
}

impl<F: IsFFTField> Domain<F> {
    pub fn new<A>(air: &A) -> Self
    where
        A: AIR<Field = F>,
    {
        // Initial definitions
        let blowup_factor = air.options().blowup_factor as usize;
        let coset_offset = FieldElement::from(air.options().coset_offset);  // h
        let interpolation_domain_size = air.trace_length();
        let root_order = air.trace_length().trailing_zeros();

        let trace_primitive_root = F::get_primitive_root_of_unity(root_order as u64).unwrap(); // w
        // [w^0, w^1, w^2, ..., w^N]
        let trace_roots_of_unity = get_powers_of_primitive_root_coset(
            root_order as u64,
            interpolation_domain_size,
            &FieldElement::one(),
        )
        .unwrap();

        let lde_root_order = (air.trace_length() * blowup_factor).trailing_zeros();
        // [hw^0, hw^1, hw^2, ..., hw^N]
        let lde_roots_of_unity_coset = get_powers_of_primitive_root_coset(
            lde_root_order as u64,
            air.trace_length() * blowup_factor,
            &coset_offset,
        )
        .unwrap();

        Self {
            root_order,
            lde_roots_of_unity_coset,
            trace_primitive_root,
            trace_roots_of_unity,
            blowup_factor,
            coset_offset,
            interpolation_domain_size,
        }
    }
}
```

`root_order`表示`trace_length`可以被2整除多少次，这里用到`trailing_zeros()`表示如果用2进制来表示可以有多少个0，举例来说

```rust
fn main() {
    let x: u32 = 8; // Binary: 1000
    let y: u32 = 16; // Binary: 10000
    let z: u32 = 7;  // Binary: 111

    println!("{}", x.trailing_zeros()); // Output: 3
    println!("{}", y.trailing_zeros()); // Output: 4
    println!("{}", z.trailing_zeros()); // Output: 0
}
```

可以看到对于偶数来说他总是末尾会有多个0，因此统计末尾有多少个0就可以知道一个数可以被2整除多少次了。这样我们就很容易知道一个数的order，也就是 $2^k$ 中的k，而一般的方法比如1024需要除2做10次运算才能知道答案，显然，用二进制数一眼就能看出来答案，计算更高效。

来看一下如何计算primitive root of unity，也就是w。

$$
w = g^{{p-1}/n}
$$

其中 $n=2^k$ 也就是想要生成几个root of unity。其背后的思想来自 $w^n=1$ 也就是

$$
w^n = g^{{p-1}/n \cdot n} = g^{p-1} = 1
$$
为什么 $g^{p-1}=1$ 这个内容来自费马小定理。这样通过选n就控制了w的生成。可以生成程序需要范围的root of unity。在实际程序中generator也就是之前算式中的g是固定的，我们通过n来调整w到底是多少。


`coset_offset`表示coset的offset偏移值，也就是来自 $hw$ 的 $h$ ，具体定义的赋值来自最开始的`test_prove_fib()`。

```rust
let proof_options = ProofOptions::default_test_options();

/// Default proof options used for testing purposes.
/// These options should never be used in production.
pub fn default_test_options() -> Self {
	Self {
		blowup_factor: 4,
		fri_number_of_queries: 3,
		coset_offset: 3,
		grinding_factor: 1,
	}
}
```

`fri_number_of_queries`表示执行fri的时候要检查几个数据。

`trace_primitive_root`根据`root_order`来生成对应的 $w$ ，该字段表示 $w$ ，是一个值。

```rust
let trace_primitive_root = F::get_primitive_root_of_unity(root_order as u64).unwrap();
```

`trace_roots_of_unity`表示main trace的domain也就是 $[w^0, w^1, w^2, ...]$

`lde_root_order`表示lde trace多项式的阶（和length不完全一致，有的时候为了一些目的会在trace后面补一些`0`来保证不同trace之间长度一致，所以不能单纯看length），计算方式是通过原始的main trace的length乘以`blowup_factor`来达到扩列的目的。

`lde_roots_of_unity_coset`表示lde trace的domain也就是 $[hw^0, hw^1, hw^2, ...]$

`interpolation_domain_size`表示trace的length，下述代码具体区分order和length之间的区别联系

```rust
let interpolation_domain_size = air.trace_length();
let root_order = air.trace_length().trailing_zeros();
```

### round 1 

**round 1.1**

解释一下`round_1_randomized_air_with_preprocessing`函数。

首先进入`interpolate_and_commit`用于生成trace表对应的多项式，以及LDE的evaluation的点，多个trace所batch的merkle tree以及root。深入看一下这个函数里面。

在函数内部，首先生成多项式以及LDE（low degree extension）。随后执行[bit reversal permutation](https://en.wikipedia.org/wiki/Bit-reversal_permutation)。这么做的目的是为了将来verifier去做checking FRI layer的时候计算方便。具体来说 verifier去做下一层的计算的时候需要之前的一些奇数项和偶数项：

$$
P_{i+1}(x^2) = \frac{P_i(x) + P_i(-x)}{2} + \beta \frac{P_i(x) - P_i(-x)}{2x}
$$

但是如果按照默认顺序构建Merkle树期结构大概类似

$$
[p(h), p(hw), p(hw^2), p(hw^3), p(hw^4), p(hw^5), p(hw^6), p(hw^7)]
$$

并不是严格按照一个数和他的相反数来构造的。如果能够按照上述公式中的相反数来构造整个merkle树，将来验证的时候计算就会简单很多。

事实上根据root of unity的计算特性，一个数和他对应的相反数就在上述的8个数中。我们要找 $p(hw)$ 所对应的相反数 $p(-hw)$ 而根据root of unity特性，其值就是 $p(hw^{i+2^{k-1}})$ 此处 k=3，因此如果按照相反数排列的话，其新排列的顺序应该是

$$
[p(h), p(hw^4), p(hw), p(hw^5), p(hw^2), p(hw^6), p(hw^3), p(hw^7)]
$$

这样的话，就符合之前的一个数与之相反数一一对应的关系。实际运算是可以通过对之前排列的每一个index值做bit reverse运算，然后可以得到下述的一个数与之相反数对应的关系。

$$
[p(h), p(hw^4), p(hw^2), p(hw^6), p(hw), p(hw^5), p(hw^3), p(hw^7)]
$$

这是bit-reverse运算的特性，刚好可以用在这里去构造两两对应的相反数。

接下来是对整个表做矩阵的矩阵转置`columns2rows`，目的是将来做batch commit操作。举例来说，原始的表是这样：

| poly a   | poly b   | poly c   |
| -------- | -------- | -------- |
| $y_{a0}$ | $y_{b0}$ | $y_{c0}$ |
| $y_{a1}$ | $y_{b1}$ | $y_{c1}$ |
| $y_{a2}$ | $y_{b2}$ | $y_{c2}$ |

转置之后变为：

| poly a | $y_{a0}$ | $y_{a1}$ | $y_{a2}$ |
| ------ | -------- | -------- | -------- |
| poly b | $y_{b0}$ | $y_{b1}$ | $y_{b2}$ |
| poly c | $y_{c0}$ | $y_{c1}$ | $y_{c2}$ |

但是batch合并merkle树的时候还是按照列来去做，也就说会对每一列元素（比如第一列）执行 $(y_{a0} || y_{b0} || y_{c0})$ 这样就可以把多个merkle树合并成一个。随后生成对应的merkle树和root。

最后将merkle树root添加到`transcript`中，至此round 1.1完成。

**round 1.2**

从`interpolate_and_commit`函数出来，继续看`round_1_randomized_air_with_preprocessing`函数。1.2主要构造RAP，以`fibonacci_rap`为例，接下来进入到该文件中。

RAP称之为[Randomized AIR with Preprocessing](https://hackmd.io/@aztec-network/plonk-arithmetiization-air)，randomized主要是在计算过程中verifier会给prover一个随机数，防止prover作弊。但是Preprocessing这个词是有点误导含义，因为该计算所生产的约束表很有可能是根据之前对应的table所动态生成的，不一定是提前定义好（PAIR确实是完全提前定义好的）。

说一下PAIR，blog里面说的很明确，搞出来一个c来试图模拟乘法和加法运算，这样我还是逐行的做约束，跨行的数据或者是乘法关系或者是加法关系。这样就模拟了PLONK的运算规则了。

至于permutation check其实和plonk的乘法与加法运算没有关系，RAP的例子中第三列就是permutation的那个辅助数据列。lookup也可以在辅助数据上做lookup。然后可能一般的电路程序可能会说哪个地方lookup，哪个地方不lookup，这个时候就可以使用PAIR中的c的0和1，来表示横着的数据哪个需要lookup，哪个不需要了。

- **PAIRs**: Fully precomputed constraints known to both parties before proving starts.
- **RAPs**: Combine an initial setup with dynamic, randomness-driven constraints introduced by the verifier to enhance security and flexibility.

总的来说RAP就是新增一个或者几个对原有table的约束列，这样可以更灵活的组织整个约束系统结构。

接下来进入到`build_auxiliary_trace`函数，该函数就是做了一个新的permutation z的trace（详见：[Randomized AIR with Preprocessing](https://hackmd.io/@aztec-network/plonk-arithmetiization-air)）代码来自`fibonacci_rap.rs`，因为simple fibonacci没有提供该方法。permutation check的z trace和文章中介绍的内容一致。

然后退回来继续看`round_1_randomized_air_with_preprocessing`函数。有了aux多项式trace之后（z），随后进入到`interpolate_and_commit`函数，同1.1一样，生成对应的多项式以及LDE的evaluation point，merkle树等，随后将数据整理发送出去。

### round 2

round 2目标是要构造composition polynomial。composition polynomial的作用是将多个约束合并成一个约束，来保证验证工作变得简洁。

首先来看一下`num_boundary_constraints`如何生成，在`fibonacci_rap`中定义了具体的`boundary_constraints`方法，在该方法中直接定义了`a0, a1, a0_aux`的值并将其添加到了`BoundaryConstraints`中，所以也就很容易知道他的length。

```rust
BoundaryConstraints::from_constraints(vec![a0, a1, a0_aux])
```

后面的`num_transition_constraints`来自

```rust
let num_transition_constraints = air.context().num_transition_constraints;
```

而air的生成又来自round1之前

```rust
let air = A::new(main_trace.n_rows(), pub_inputs, proof_options);

fn new(
	trace_length: usize,
	pub_inputs: &Self::PublicInputs,
	proof_options: &ProofOptions,
) -> Self {
	let transition_constraints: Vec<
		Box<dyn TransitionConstraint<Self::Field, Self::FieldExtension>>,
	> = vec![
		Box::new(FibConstraint::new()),
		Box::new(PermutationConstraint::new()),
	];

	let context = AirContext {
		num_transition_constraints: transition_constraints.len(),
	};
```

可见`num_transition_constraints`来自两部分，一部分是`FibConstraint`另外一部分是aux的`PermutationConstraint`，所以目前length应该是2。

继续往后看，接下来计算coefficients，这个系数是指的 $\beta_k^T$ 和 $\beta_j^B$ ，所以之前要去计算`num_boundary_constraints`和`num_transition_constraints`这样好分配随机项的值。所以后续有代码就是做随机项的具体分配工作。从代码中也可以看出，随机项实际上就是 $[\beta, \beta^2, \beta^3 ...]$

```rust
let mut coefficients: Vec<_> =
	core::iter::successors(Some(FieldElement::one()), |x| Some(x * &beta))
		.take(num_boundary_constraints + num_transition_constraints)
		.collect();

let transition_coefficients: Vec<_> =
	coefficients.drain(..num_transition_constraints).collect();
let boundary_coefficients = coefficients;
```

做完准备工作接下来进入到`round_2_compute_composition_polynomial`看一下round 2具体处理过程。

在该代码中因为具体实现的可能有所不同，还是以`fibonacci`为例，此处的evaluate事实上只是把public的`a[0]`的值做了一个赋值，即是`evalutor.boundary_constraints==a[0]`

```rust
let evaluator = ConstraintEvaluator::new(air, &round_1_result.rap_challenges);
```

接下来进入`evaluator.evaluate`函数，获得对应的y值为将来生成H多项式做准备。具体进入到`evaluator.evaluate`函数看一下。

boundary这块计算最终是要构造

$$
B_j = \frac{t_j - P_j^B}{Z_j^B}
$$

其中 $Z_j^B$ 表示 boundary约束所对应的消失多项式， $P_j^B$ 表示约束多项式。首先来看一下分母是如何构造的。

```rust
let boundary_zerofiers_inverse_evaluations: Vec<Vec<FieldElement<A::Field>>> =
	boundary_constraints
		.constraints
		.iter()
		.map(|bc| {
			let point = &domain.trace_primitive_root.pow(bc.step as u64);
			let mut evals = domain
				.lde_roots_of_unity_coset
				.iter()
				.map(|v| v.clone() - point)
				.collect::<Vec<FieldElement<A::Field>>>();
			FieldElement::inplace_batch_inverse(&mut evals).unwrap();
			evals
		})
		.collect::<Vec<Vec<FieldElement<A::Field>>>>();
```

  $Z_j^B$ 消失多项式可以表示成

$$
Z_j^B = \prod_{i=0}^n(cosset_x-w^{a_i})
$$

在类似PLONK这样的协议上，我们其实主要构造多项式，然后在一点打开就行了，所以可以保留整个多项式，只在某一点打开的时候才去做具体的  $Z_j^B$ 值的计算，但是像STARK这样要构造Merkle树，所以就不得不提前在所有可能的验证点（LDE trace）都提前打开算一遍，有这些值将来才好构建整个树特别是发送root。因此上述代码看起来有一点奇怪为什么是两次map就在于不仅要对所有的 $w^{a_i}$ 去做计算，同时还要对所有的 $x$ 也做计算。但是这样算完格式有一点奇怪，在不说inverse的情况下，最后结果大概类似

$$
[[(h-a_0)(hw-a_0)(hw^2-a0)...], [(h-a_1)(hw-a_1)(hw^2-a1)...], ...]
$$

将来真的去使用的时候还需要对值做一些排序等处理。

继续向后看代码。`boundary_polys_evaluations`这个计算的是 $P_j^B$ 的评估。

```rust
let boundary_polys_evaluations = boundary_constraints
	.constraints
	.iter()
	.map(|constraint| {
		if constraint.is_aux {
			(0..lde_trace.num_rows())
				.map(|row| {
					let v = lde_trace.get_aux(row, constraint.col);
					v - &constraint.value
				})
				.collect_vec()
		} else {
			(0..lde_trace.num_rows())
				.map(|row| {
					let v = lde_trace.get_main(row, constraint.col);
					v - &constraint.value
				})
				.collect_vec()
		}
	})
	.collect_vec();
```

参考[Diving DEEP FRI in the STARK world](https://blog.lambdaclass.com/diving-deep-fri/) 中boundary constraint部分来作为符号系统说明一下上述代码。

文中的boundary constraint是a(1)=3，文中的约束多项式写为t(x)，所以boundary constraint是 $p_1(x)=t(x)-3$  ，但是在实际的编程中，如之前所说，要构建整个merkle树要evaluate所有的点，所以我们并不是先算出 $t(x)$ 然后算 $p_1(x)$ 而是直接拿t(x)的y值去做差（因为实际计算都是在coset层面，所以这里的y是t(coset)对应的y，一开始就把这个表构建起来了，所以只需要查表就行）计算出来的 $p_1$ 也不是多项式，而是评估点。（此处说的 $p_1$ 也是之前公式例子中的 $t_j - P_j^B$ ）后面都是直接用评估点去做计算而不是差值算出来，全部用评估点算完会给一个评估点列表，然后H用这个评估点列表再插值的方式把H算出来，这样只需要插值一次，而不用在每个约束计算的时候都去做插值。

代码中的`v`对应的是t(coset)对应的y，而`constraint.value`可类比为之前举例子中的`3`。

table表可能会有多个t，每一个t都来一遍评估计算，所以最终得到的`boundary_polys_evaluations`类型会是`Vec<Vec>`。

继续向后看，`boundary_evaluation`是将之前的一些内容拼起来，构造

$$
\sum_j \beta_j^B B_j
$$
在看具体构造代码之前先看一下简单的map fold组合的运算逻辑

```rust
fn main() {
    let nums = vec![1, 2, 3, 4];
    let coefficients = vec![2, 3, 4, 5];

    let results: Vec<_> = nums.iter()
        .map(|&num| {
            (0..4).zip(&coefficients)
                .fold(0, |acc, (index, &coef)| {
                    acc + num * coef
                })
        })
        .collect();

    println!("{:?}", results); // Should print [14, 28, 42, 56]
}

```

For `num = 1`: 1∗2+1∗3+1∗4+1∗5=14。

具体看一下代码

```rust
let boundary_eval_iter = 0..domain.lde_roots_of_unity_coset.len();
let number_of_b_constraints = boundary_constraints.constraints.len();

let boundary_evaluation: Vec<_> = boundary_eval_iter
	.map(|domain_index| {
		(0..number_of_b_constraints)
			.zip(boundary_coefficients)
			.fold(FieldElement::zero(), |acc, (constraint_index, beta)| {
				acc + &boundary_zerofiers_inverse_evaluations[constraint_index]
					[domain_index]
					* beta
					* &boundary_polys_evaluations[constraint_index][domain_index]
			})
	})
	.collect();
```

从`boundary_eval_iter`可以知道，开始迭代时使用的长度是和coset评估值长度一致，之前也说道`boundary_zerofiers_inverse_evaluations`使用时要挑出值来做计算，最后得到的是evaluate取不同的coset之后得到的boundary评估的**列表**。之前公式说的加和是指的对同一个evaluate点比如 $hw$ 或者 $hw^2$ 如果有多个boundary constraint的话那么把他们加和，并不是指对所有不同的evaluate点统共算在一起做一个加和。

继续向后看代码，`evaluations_t_iter`和之前`boundary_eval_iter`一样，长度是coset评估值长度。

在往后看代码之前，首先介绍一个新的概念叫“**frame**”，frame有structure的意思，evaluate frame表示一些特殊的评估点。比如原始的lde trace是 $[b_0, b_1, b_2, b_3, b_4, b_5, b_6, b_7]$ 并且lde step 是2那么evaluation frame可能是 $[b_0, b_2, b_4, b_6]$ ，frame的主要作用是用来收集lde step（或者说是lde evaluate的值），这些step是用于做transition constraint所必须的step。

从代码具体看一下frame的定义

```rust
pub struct Frame<'t, F: IsSubFieldOf<E>, E: IsField>
where
    E: IsField,
    F: IsSubFieldOf<E>,
{
    steps: Vec<TableView<'t, F, E>>,
}
```

收集step形成frame的方式如下

```rust
pub fn read_from_lde(
	lde_trace: &'t LDETraceTable<F, E>,
	row: usize,
	offsets: &[usize],
) -> Self {
	let blowup_factor = lde_trace.blowup_factor;
	let num_rows = lde_trace.num_rows();
	let step_size = lde_trace.lde_step_size;

	let lde_steps = offsets
		.iter()
		.map(|offset| {
			let initial_step_row = row + offset * step_size;
			let end_step_row = initial_step_row + step_size;
			let (table_view_main_data, table_view_aux_data) = (initial_step_row..end_step_row)
				.step_by(blowup_factor)
				.map(|step_row| {
					let step_row_idx = step_row % num_rows;
					let main_row = lde_trace.get_main_row(step_row_idx);
					let aux_row = lde_trace.get_aux_row(step_row_idx);
					(main_row, aux_row)
				})
				.unzip();

			TableView::new(table_view_main_data, table_view_aux_data)
		})
		.collect_vec();

	Frame::new(lde_steps)
}
```

该函数传进来的trace是round 1 生成的trace，需要注意的是，这个trace是上文中的原始列表，并不是转置之后的列表（batch只是用来生成merkle树的时候用一下，平时程序里面执行的时候使用的是原始版本）。

我现在不理解frame为什么要搞成这个形式，或许是frame实际上是处理一些数据，保证每个约束项的数据是对应数据，什么意思，比如这里面每一个`TableView::new(table_view_main_data, table_view_aux_data)`的数据都是从trace中得到的，但是原始的trace有boundary constraint和transition constraint的数据，这里面要处理剔除掉boundary constraint数据，还有就是比如对于fibonacci而言，有 $a_{n+2}=a_{n+1}+a_n$ 那么，$a_{n+2}$ 就不能让n取道最后一个位置，所以frame会考虑step步长。

看一下该例子中的evaluate具体如何做的

```rust
fn evaluate(
	&self,
	frame: &Frame<F, F>,
	transition_evaluations: &mut [FieldElement<F>],
	_periodic_values: &[FieldElement<F>],
	_rap_challenges: &[FieldElement<F>],
) {
	let first_step = frame.get_evaluation_step(0);
	let second_step = frame.get_evaluation_step(1);
	let third_step = frame.get_evaluation_step(2);

	let a0 = first_step.get_main_evaluation_element(0, 0);
	let a1 = second_step.get_main_evaluation_element(0, 0);
	let a2 = third_step.get_main_evaluation_element(0, 0);

	let res = a2 - a1 - a0;

	transition_evaluations[self.constraint_idx()] = res;
}
```

fibonacci例子中transition constraint用公式表示的是

$$
C(x) = \frac{t(xg^2) - t(xg) - t(x)}{\prod_{i=0}^n(x-g^i)}
$$

fibonacci例子中并没有真的用到`periodic_values`所以关于periodic部分也暂时不做分析。

我暂时不理解上述代码到底是怎么做到的，但总的来说，应该是得到一个数值。然后有下述函数：

```rust
fn compute_transition_prover(
	&self,
	frame: &Frame<Self::Field, Self::FieldExtension>,
	periodic_values: &[FieldElement<Self::Field>],
	rap_challenges: &[FieldElement<Self::FieldExtension>],
) -> Vec<FieldElement<Self::FieldExtension>> {
	let mut evaluations =
		vec![FieldElement::<Self::FieldExtension>::zero(); self.num_transition_constraints()];
	self.transition_constraints()
		.iter()
		.for_each(|c| c.evaluate(frame, &mut evaluations, periodic_values, rap_challenges));

	evaluations
}
```

evaluate首先根据`transition_constraints`长度做出一堆的`0`初始化，然后在迭代时按照`self.constraint_idx()`去填之前的初始化的`evaluations`，最后将其返回，得到的就是 C(x)分子部分的lde评估。在外部有一个循环，有一个新frame，随后就`compute_transition_prover`运算得到一个对应的评估，举个例子以之前的fibonacci公式为例，比如frame此时对应的点是 $h$ 那么`c.evaluate`一次运算会得到一个评估点也就是 $t(hw^2)-t(hw)-t(h)$ ，t是一个约束，如果还有一个别的约束方程比如s，那么也会同样的算一遍。最终会得到多个约束在h点的评估值，比如标识为 [t(h), s(h)]。此处的fibonacci并没有额外的s约束。

然后迭代不同的frame，就会得到不同的约束，最终将其放在`evaluations_transition`中，`evaluations_transition`表示类似于 $[[t(h), s(h)], [t(hw), s(hw)], [t(hw^2), s(hw^2)]...]$

回到最外面的evaluate函数，目前`evaluations_transition`已经得到了对应的transaction评估点。

```rust
let evaluations_transition =
                    air.compute_transition_prover(&frame, &periodic_values, rap_challenges);
```

接下来要构造的是

$$
\sum_k \beta_k^TC_k
$$

同之前的boundary evaluation一样，是一个**列表**，对于同一个评估点，约束会加和，但并不是对不同的评估做一个累加。对每一个评估点，都有`acc_transition + boundary`也就是将该评估点的boundary constraint和transition constraint加和，形如公式

$$
H = \sum_k \beta_k^TC_k + \sum_j \beta_j^B B_j
$$

最终整个`evaluate`函数返回H评估点的列表，也即 $h$ $hw$ $hw^2$ ... 等评估点的H列表。

向上回到`round_2_compute_composition_polynomial`函数继续向后看，`composition_poly`是根据之前的H评估点生成对应的多项式。随后可以通过下述代码将原有的H多项式拆成多个小的

```rust
let number_of_parts = air.composition_poly_degree_bound() / air.trace_length();
let composition_poly_parts = composition_poly.break_in_parts(number_of_parts);
```

比如`number_of_parts`是2的话，会有

$$
H = H_1(X^2) + XH_2(X^2)
$$

这样看好像没有什么意义，发送一个大的完整H和发送两个小的H proof 大小不会有变化，验证一个H和验证两个尺寸砍半的H从验证时间来看也没有变化，好像没有意义，但是这种想法是基于串行思维考虑，拆成两个独立的小H，可以去做到并行计算（计算逻辑完全一致，互相独立互不干扰），如果并行计算的话，确实计算时间可以减半。所以这么做从计算优化角度考虑有一定价值。

`lde_composition_poly_parts_evaluations`表示对拆分之后的 $H_1$ 和 $H_2$ 生成evaluation，是一个数组。

接下来进入到`commit_composition_polynomial`用于生成 $H_1$ 和 $H_2$ 的merkle树和merkle树根，树根称之为**commitment**表示为 $[H_1]$ 和 $[H_2]$ 。具体看一下代码，代码实现中有一些运算的优化设计。

在该函数中，首先构造出来`[[H1(h), H2(h)], [H1(hw), H2(hw)]...]`这样的数据结构，存储在`lde_composition_poly_evaluations`中，随后做bit-inverse来做成两两配对，方便将来做奇偶（对称点）运算随后将奇偶（对称点）放在一组里，类似这样 $[[H_1(h), H_2(h), H_1(hw^4), H_2(hw^4)]...]$ （这是按之前那个8个数的例子），随后对这样整个列的表做merkle树

也就是说目前最后生成的还是一个merkle树，而不是 $H_1$ 和 $H_2$ 分别生成merkle树，不过看上述的列表，可以发现，相比于原始的一个大的H，这样的一个merkle树的尺寸确实减半了。

最后将merkle树根添加到transcript里面。

### round 3

round3主要做的是在一个z点将多项式打开，做评估，用于将来做多项式约束性的验证工作。这个z的选取要求不能是domain中的元素也不能是lde coset中的元素，也就是既不能是 $[w^0, w^1, w^2, ...]$ 中元素也不能是 $[hw^0, hw^1, hw^2, ...]$ 中元素。

随后进入`round_3_evaluate_polynomials_in_out_of_domain_element`函数，首先生成 $z^n$ 这个n来自`composition_poly_parts.len()`也就是 $H_1$ 或 $H_2$ 的length。$z^n$ 将作为最终的评估点。

`composition_poly_parts_ood_evaluation`表示`composition_poly_parts_ood_evaluation`在 $z^n$ 上的评估，也就是 $[H_1(z^n), H_2(z^n)]$ 。（odd是out-of-domain）

`trace_ood_evaluations`表示（以fibonacci为例）是 $[t(z), t(z \cdot g), t(z \cdot g^2)]$

最后将上述几个评估值 $[H_1(z^n), H_2(z^n)]$ $[t(z), t(z \cdot g), t(z \cdot g^2)]$ 添加到transcript中。

### round 4

round 4主要是关于FRI的，构造deep composition polynomial并对其做FRI操作。deep composition polynomial形如

$$
p_0 = \gamma \frac{H_1 - H_1(z^n)}{X-z^n} + \gamma' \frac{H_2 - H_2(z^n)}{X-z^n} + \sum_j (\gamma_j \frac{t_j - t_j(z)}{X-z} + r_j' \frac{t_j - t_j(gz)}{X-gz})
$$

进入到`round_4_compute_and_run_fri_on_the_deep_composition_polynomial`函数中。

从`deep_composition_coefficients`代码可以知道，这些 $\gamma$ 的值就是首先从transcript产生出来一个，然后后续的 $\gamma$ 参数实际上是平方关系，也就是需要多少个 $\gamma$ 就在后面生成多少个带平方的，举个例子可以有一个列表 $[\gamma, \gamma^2, \gamma^3, ...]$ 需要多少个就从列表中拿多少个。

随后生成`deep_composition_poly`也就是上述公式中的 $p_0$ ，进入到`compute_deep_composition_poly`函数中。首先生成关于H的多项式，这个比较简单，然后是关于t的，需要注意的是这个t是所有的都要做一遍，也包含aux的t。

```rust
let trace_terms =
	// trace_polys include main trace and aux trace 
	trace_polys
		.iter()
		.enumerate()
		.fold(Polynomial::zero(), |trace_terms, (i, t_j)| {
			// i is index t_j is trace poly
			Self::compute_trace_term(
				&trace_terms,
				(i, t_j),
				trace_frame_length,
				trace_terms_gammas,
				&trace_frame_evaluations.columns(),
				transition_offsets,
				(z, primitive_root),
			)
		});
```

关于fold函数，如果没有之前的`enumerate`，那么使用的时候只能是放入 `|trace_terms, t_j|`而不是现在的index形式，正式因为加入了`enumerate`才使得加入index成为可能。

现在 $p_0$ 有了。

**round 4.1 FRI commit and query phase**

继续向后看代码，进入到`commit_phase`函数。该函数的主要作用是把每一层的FRI layer都添加到一个列表里，以及计算出最后一层的FRI layer（此时不是一个layer而是一个数了）。

```rust
pub fn commit_phase<F: IsFFTField + IsSubFieldOf<E>, E: IsField>(
    number_layers: usize,
    p_0: Polynomial<FieldElement<E>>,
    transcript: &mut impl IsTranscript<E>,
    coset_offset: &FieldElement<F>,
    domain_size: usize,
) -> (
    FieldElement<E>,
    Vec<FriLayer<E, BatchedMerkleTreeBackend<E>>>,
)
where
    FieldElement<F>: AsBytes + Sync + Send,
    FieldElement<E>: AsBytes + Sync + Send,
{
    let mut domain_size = domain_size;

    // number_layers: domain.root_order
    let mut fri_layer_list = Vec::with_capacity(number_layers);
    let mut current_layer: FriLayer<E, BatchedMerkleTreeBackend<E>>;
    let mut current_poly = p_0;

    let mut coset_offset = coset_offset.clone();  // h

    for _ in 1..number_layers {
        // <<<< Receive challenge 𝜁ₖ₋₁
        let zeta = transcript.sample_field_element();
        // for generate next evaluation domain
        coset_offset = coset_offset.square();  // h^2
        domain_size /= 2;

        // Compute layer polynomial and domain
        current_poly = FieldElement::<F>::from(2) * fold_polynomial(&current_poly, &zeta);
        current_layer = new_fri_layer(&current_poly, &coset_offset, domain_size);
        let new_data = &current_layer.merkle_tree.root;
        // TODO: remove this clone
        fri_layer_list.push(current_layer.clone()); 

        // >>>> Send commitment: [pₖ]
        transcript.append_bytes(new_data);
    }

    // <<<< Receive challenge: 𝜁ₙ₋₁
    let zeta = transcript.sample_field_element();

    let last_poly = FieldElement::<F>::from(2) * fold_polynomial(&current_poly, &zeta);

    let last_value = last_poly
        .coefficients()
        .first()
        .unwrap_or(&FieldElement::zero())
        .clone();

    // >>>> Send value: pₙ
    transcript.append_field_element(&last_value);

    (last_value, fri_layer_list)
}
```

在for循环中，`current_poly`表示做一次折叠运算之后新的poly，也就是

$$
p_k = p_{k-1}^{odd}(X) + \zeta_{k-1}p_{k-1}^{even}(X)
$$

**我暂时不明白为什么此处要做一个乘2的操作，结果发现答案在verifier这里，看一下verifier的代码

```rust
// Reconstruct p₁(𝜐²)
let mut v =
	(p0_eval + p0_eval_sym) + evaluation_point_inv * &zetas[0] * (p0_eval - p0_eval_sym);
```

可以看到verifier这里计算的时候没有做除2计算，之所以能这么做是因为prover给的p1...pn的数据都乘2了，是2倍的，所以verifier不需要做除法运算了，这算是一个超级小的优化内容，加速了verifier的计算速度，要不然verifier还需要做一个除法计算。

来看一下`fold_polynomial`，该函数的作用是生成 $p_k$ 的多项式系数表示。

来看一下`new_fri_layer`，如何构建一个新的fri layer层。注意如果说之前的 $p_{k-1}$ 输入的变量是x的话，那么新的 $p_{k-1}(y=x^2)$ 的定义域发生了变化，由之前的x变成了 $x^2$ 所以如果要直接套用之前的domain去计算的话，就会出错，要对domain重新做一些计算。之前的代码有`domain_size /= 2;`其实也是说定义域的尺寸缩减了。进入到`new_fri_layer`具体看一下代码。

```rust
pub fn new_fri_layer<F: IsFFTField + IsSubFieldOf<E>, E: IsField>(
    poly: &Polynomial<FieldElement<E>>,
    coset_offset: &FieldElement<F>,
    domain_size: usize,
) -> crate::fri::fri_commitment::FriLayer<E, BatchedMerkleTreeBackend<E>>
where
    FieldElement<F>: AsBytes + Sync + Send,
    FieldElement<E>: AsBytes + Sync + Send,
{
    let mut evaluation =
        // TODO: return error
        Polynomial::evaluate_offset_fft(poly, 1, Some(domain_size), coset_offset).unwrap(); 

    in_place_bit_reverse_permute(&mut evaluation);

    let mut to_commit = Vec::new();
    for chunk in evaluation.chunks(2) {
        to_commit.push(vec![chunk[0].clone(), chunk[1].clone()]);
    }

    let merkle_tree = BatchedMerkleTree::build(&to_commit);

    FriLayer::new(
        &evaluation,
        merkle_tree,
        coset_offset.clone().to_extension(),
        domain_size,
    )
}
```

在`evaluate_offset_fft`这块对domain数据要做一点特殊处理，不能直接向原始的直接domain就完了。

```rust
pub fn evaluate_offset_fft<F: IsFFTField + IsSubFieldOf<E>>(
	poly: &Polynomial<FieldElement<E>>,
	blowup_factor: usize,
	domain_size: Option<usize>,
	offset: &FieldElement<F>,
) -> Result<Vec<FieldElement<E>>, FFTError> {
	let scaled = poly.scale(offset);
	Polynomial::evaluate_fft::<F>(&scaled, blowup_factor, domain_size)
}
```

看一下代码，它首先scale了一下。我们从数学角度解释一下，原始的domain是 $[hw^0, hw, hw^2, ...]$ 新的domain $X^2=(hw)^2$（注意尺寸减半）按照原有的符号表述可以写为 $[(hw^0)^2, (hw)^2, (hw^2)^2, ...]$ ，注意只要前面的一半domain数据即可。从上述domain的改变可以发现，相比原始的domain，新的domain每一项都多乘了一个h，所以实际代码时有`let scaled = poly.scale(offset);`，对原有的poly系数对应的扩展。

接下来是要解决w的平方能力。在root of unity中，如果domain的尺寸减半，那么实际每一项就是对应原来项的平方，举个例子，比如说有domain尺寸为8，有domain为 $[1, w, w^2, w^3, w^4, w^5, ...]$ ，如果此时把domain尺寸减半，新的domain用c来表示 $[1, c, c^2, c^3]$ 此时 c就是 $w^2$，$c^2$ 就是 $(w^2)^2$ 刚好满足上述的对应关系。所以只需要分两步，第一步是做scale乘出h，第二步是domain尺寸减半后代码，就可以构造出来 $[(hw^0)^2, (hw)^2, (hw^2)^2, ...]$ 也就是 $X^2$ 的评估而不是原来的X了。

用数学的方式稍微表示证明一下，还是用8和4为例子，原始的8，用w，新的c用4

$$
\begin{split}
w &= cos\frac{2\pi}{8} + isin\frac{2\pi}{8} \\
w^2 &= cos\frac{2\pi}{4} + isin\frac{2\pi}{4} \\
c &= cos\frac{2\pi}{4} + isin\frac{2\pi}{4}
\end{split}
$$

证明完成上述构造方式ok。

通过之前的分析，现在`evaluation`已经表示  $p_{k-1}$ 在 $y=x^2$ 的评估了。

```rust
let mut evaluation =
	// TODO: return error
	Polynomial::evaluate_offset_fft(poly, 1, Some(domain_size), coset_offset).unwrap(); 
```

接下来继续向后看，接下来对评估做bit-reverse。使其奇偶（对称点）排列，方便将来验证。再之后构造merkle树，需要注意的是每个叶子结点都是由一组对称点共同组成即 $(p_{k-1} || -p_{k-1})$ 最后将merkle树，domain size（已减半，对应实际的该evaluate的domain）以及其他该fri layer相关信息添加进来。

回到之前的代码，逐层构造完fri layer，获得最终的一个数值 $p_n$ ，将layer以及数值信息返回，至此4.1完成。

需要注意的是layer是一个list，即`fri_layer_list`，里面存放着每一层的p的merkle tree。将来给evaluation postion的时候，需要逐层遍历`fri_layer_list`然后给每一层对应的merkle tree的位置position proof。

**round 4.2 Grinding**

grinding是一个新概念，首先浏览一下[StarkDEX Deep Dive: the STARK Core Engine](https://medium.com/starkware/starkdex-deep-dive-the-stark-core-engine-497942d0f0ab) 关于grinding的内容。

来说一下grind的具体步骤，比如说我们要求去计算一个返回值 $c = hash(commitments || nonce)$ 这个c最后几位是`0`，要通过找nonce的方式实现这一点就是一种POW运算。这么做的目的是诚实的prover只需要去做一次这样的运算，而对于恶意的prover，它做了一个虚假的commitment然后被verifier拒绝后，它下次提交新的证明时还需要再来做一次这样的POW运算，一定程度上增加了它作弊的开销，增强整个系统的安全性。不过这样的POW被拒然后再算，时间开销时线性的，其实也还好，增加的总体的安全性有限。

算出nonce（nonce用y表示），然后将nonce添加到transcript中。

**round 4.3 FRI query phase**

这一部分内容是开始一些点，方便将来verifier去做验证。

`number_of_queries`表示想要从lde中验证多少个点。`iotas`表示lde中想要验证点的index索引集合（希腊字母 $\iota$）。

```rust
let iotas = Self::sample_query_indexes(number_of_queries, domain, transcript);

fn sample_query_indexes(
	number_of_queries: usize,
	domain: &Domain<A::Field>,
	transcript: &mut impl IsTranscript<A::FieldExtension>,
) -> Vec<usize> {
	let domain_size = domain.lde_roots_of_unity_coset.len() as u64;
	(0..number_of_queries)
		.map(|_| (transcript.sample_u64(domain_size >> 1)) as usize)
		.collect::<Vec<usize>>()
}
```

从代码中可以看出，iotas取值范围是`[0, led domain size/2)`，之所以除以2是因为选的点都应该是两两对称的，如果在整个domain上去随机选择点的话，就有可能选到两个点`[s, -s]`，但实际上并不是这样，如果我们想选2个点`[a, b]`，实际最终期望的是`[a, -a, b, -b]`，所以我们最开始在domain size/2上去选，最后计算的时候再乘2，也就是假设最开始不幸选的两个点挨在一起如`[s, -s]`，通过乘2的方式也能将这两个值拉开，每一个新的cell都可以再有一个填充位，最后构成形如`[a, -a, b, -b]`的形式。

随后进入`query_phase`函数。

该函数是对每一层的fri layer计算 $Open(p_k(D_k), v_s^{2^k})$ 以及 $Open(p_k(D_k), -v_s^{2^k})$ 也就是其对称点的值，方便将来做计算。首先来说一下merkle树，在该实现中，`nodes[0]`表示merkle tree root，然后是`nodes[1]`表示第二层，以此类推，最下面一层是叶子结点。来看一下`query_phase`的代码。

```rust
pub fn query_phase<F: IsField>(
    fri_layers: &Vec<FriLayer<F, BatchedMerkleTreeBackend<F>>>,
    iotas: &[usize],
) -> Vec<FriDecommitment<F>>
where
    FieldElement<F>: AsBytes + Sync + Send,
{
    if !fri_layers.is_empty() {
        let query_list = iotas
            .iter()
            .map(|iota_s| {
                let mut layers_evaluations_sym = Vec::new();
                let mut layers_auth_paths_sym = Vec::new();

                let mut index = *iota_s;
                for layer in fri_layers {
                    // symmetric element
                    let evaluation_sym = layer.evaluation[index ^ 1].clone();
                    let auth_path_sym = layer.merkle_tree.get_proof_by_pos(index >> 1).unwrap();
                    layers_evaluations_sym.push(evaluation_sym);
                    layers_auth_paths_sym.push(auth_path_sym);

                    index >>= 1;
                }

                FriDecommitment {
                    layers_auth_paths: layers_auth_paths_sym,
                    layers_evaluations_sym,
                }
            })
            .collect();

        query_list
    } else {
        vec![]
    }
}
```

之前的公式 $Open(p_k(D_k), v_s^{2^k})$ 表示有一点不妥，我们只算一侧对称点 $Open(p_k(D_k), -v_s^{2^k})$，而不是两边的点都算出来。我们会向verifier提供对称点，然后verifier根据deep composition polynomial的评估值重新构建整个FRI的计算流程，所以只需要向verifier提供一侧对称点就可以了，原始的部分（另外一侧）即 $Open(p_k(D_k), v_s^{2^k})$ 由verifier自行计算即可。还有就是`p0`和`-p0`不需要提供给verifier，verifier可以自己算出来。

`query_phase`就是计算对称点的这个过程。使用 `index ^ 1`这种bit reverse的方式可以找到index在root of unity下的对称点（因为之前生成的fri layer各层评估点没有使用bit reverse做处理，所以这里需要使用bit reverse去找对称点）。`index >> 1`等价于 `index // 2` 这么做的目的是每一轮fri layer的domain size都是减半，所以新一轮index的索引位置实际也应该减半，于是有 `index >>= 1`。

`fri_layers_merkle_roots`表示每一个fri layer层的merkle树集合。

接下来进入`open_deep_composition_poly`函数。该函数计算t以及 $H_1$ 和 $H_2$ 在评估点以及评估点的对称点打开。

从`open_trace_polys`这个函数的部分代码可以看出

```rust
let domain_size = domain.lde_roots_of_unity_coset.len();

let index = challenge * 2;
let index_sym = challenge * 2 + 1;
PolynomialOpenings {
            proof: tree.get_proof_by_pos(index).unwrap(),
            proof_sym: tree.get_proof_by_pos(index_sym).unwrap(),
            evaluations: lde_trace
                .get_row(reverse_index(index, domain_size as u64))
                .to_vec(),
            evaluations_sym: lde_trace
                .get_row(reverse_index(index_sym, domain_size as u64))
                .to_vec(),
        }
```

之前说iotas是在`[0, led domain size/2]`而在这里又乘了一个2恢复到domain size，原因分析在之前已经给出，而对于trace t还是要在完成的domain上面去找评估点。而给的proof路径也要给2个，原始的值一个，对称点一个。

构造proof，将之前的一些项打包整理发送。至此prove工作完成


## Verifier

### Step 1 Replay interactions and recover challenges

step 1就是根据prover之前的顺序重新生成一遍所有的challenge。然后验证grind阶段之前prove生成的nonce是否可以通过POW检验。

### Step 2 Verify claimed composition polynomial

进入`step_2_verify_claimed_composition_polynomial`函数看一下，verifier根据proof提供的一些evaluate的值自己重新算一下下式在各个验证点的值

$$
h = \sum_k \beta_k^Tc_k + \sum_j \beta_j^Bb_j
$$

并和prover提供的H验证点的值做比较，如果数值一直说明验证通过。

### Step 3 Verify FRI

第三步是重构deep composition polynomial以及对evaluate点做验证。进入到`reconstruct_deep_composition_poly_evaluations_for_all_queries`函数，首先看一下，该函数可以获得所有deep composition poly的评估点以及对称的评估点（即 $p_0$ 以及其对称点），方便后续做逐层验证。

在该函数中首先用一个变量`evaluations`获得之前prover提供的所有deep poly open点的评估值（原始的prover还有一些其他项，verify这里只取t），即 

```rust
// Open(tⱼ(D_LDE), 𝜐₀)
// Open(tⱼ(D_LDE), -𝜐ᵢ)
```

随后verifier自己重新构造一遍deep composition poly在对应点的评估值

```rust     
deep_poly_evaluations.push(Self::reconstruct_deep_composition_poly_evaluation(
		proof,
		&evaluation_point,
		primitive_root,
		challenges,
 		&evaluations,          &proof.deep_poly_openings[i].composition_poly.evaluations,
            ));
```

`reconstruct_deep_composition_poly_evaluation`函数就是用来获取deep composition poly在open点的评估值。对称点的评估值同理。代码中`evaluation_point` 就是指的 $v_s$ 

回到之前的`step_3_verify_fri`函数继续向后看，`evaluation_point_inverse`获得评估点的分母数组，也就是原来x的inverse形式 $\frac{1}{x}$ 。因为算式

$$
P_{i+1}(x^2) = \frac{P_i(x) + P_i(-x)}{2} + \beta \frac{P_i(x) - P_i(-x)}{2x}
$$

需要这个inverse。具体看一下代码

```rust
let mut evaluation_point_inverse = challenges
            .iotas
            .iter()
            .map(|iota| Self::query_challenge_to_evaluation_point(*iota, domain))
            .collect::<Vec<FieldElement<A::Field>>>();
        FieldElement::inplace_batch_inverse(&mut evaluation_point_inverse).unwrap();
```

代码首先对iota的数据做乘2操作，然后执行inv运算，得到新的数组。

继续向后，`verify_query_and_sym_openings`开始逐层验证FRI，一直到最后生成一个数看看是否和prover提供的数据一致，如果一致说明验证通过。

### Step 4 Verify trace and composition polynomial openings

这一步是验证之前的 Open(Hᵢ(D_LDE), 𝜐) and Open(Hᵢ(D_LDE), -𝜐) 这两个评估点是否确实来自prover真实的，而不是随便欺骗一个，采用的方式就是验证这两个点是否来自之前的merkle树，用merkle proof的方式来验证。trace验证也是同理，验证一下对应的merkle树。

至此验证工作全部结束。

#### TODO
1. 别的例子中关于periodic_values是否会用到
