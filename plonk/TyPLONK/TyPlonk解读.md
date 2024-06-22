
## Compile

假设有这样一段约束程序：

```rust
fn run<V: Var>(inputs: [V; 3]) {
	let [a, b, c] = inputs;
	let a = a.clone() * a;
	let b = b.clone() * b;
	let c = c.clone() * c;
	let d = a + b;

	d.assert_eq(&c);
}
```

这个程序是一段可执行的程序，但是并不能直接放入plonk证明中，需要首先对该程序做一定的翻译工作，将其转换成plonk可理解的形式，随后才能在zk系统中生成对应的证明。接下来首先对该程序做一定的编译工作。

总的编译过程如下所示：

```rust
pub fn compile<const I: usize, C: CircuitDescription<I>>() -> CompiledCircuit<I, C> {
	let circuit = C::run::<BuildVar>;
	let context = Context::default();
	let inputs = [(); I].map(|_| BuildVar::input(&context));
	circuit(inputs);

	let (gates, mut permutation) = context.finish();

	{
		let rows = gates.len();
		let domain = <GeneralEvaluationDomain<Fr>>::new(rows).unwrap();
		let srs = Srs::random(domain.size());
		let mut polys = [(); 5].map(|_| <Vec<Fr>>::with_capacity(rows));
		gates.into_iter().for_each(|gate| {
			let row = gate.to_row();
			polys
				.iter_mut()
				.zip(row.into_iter())
				.for_each(|(col, value)| col.push(value));
		});
		let permutation = permutation.build(rows);
		permutation.print();
		let permutation = permutation.compile();
		let scheme = KzgScheme::new(&srs);
		let polys = polys.map(|evals| {
			let poly = Evaluations::from_vec_and_domain(evals, domain).interpolate();
			let commitment = scheme.commit(&poly);
			(poly, commitment)
		});
		let commitments = polys
			.iter()
			.map(|(_, commitment)| commitment.clone())
			.collect::<Vec<_>>();
		let polys = polys.map(|(poly, _)| poly);

		let [q_l, q_r, q_o, q_m, q_c] = polys;
		let gate_constrains = GateConstrains {
			q_l,
			q_r,
			q_o,
			q_m,
			q_c,
			fixed_commitments: commitments.try_into().unwrap(),
		};
		CompiledCircuit {
			gate_constrains,
			copy_constrains: permutation,
			srs,
			domain,
			circuit_definition: PhantomData,
			rows,
		}
	}
}
```

首先通过函数式的方式把一个函数赋值给circuit，接着对context赋默认值。context使用的是`Rc<Mutex>`的方式是方便其他上下文对该值做修改，如同context的字面意思，该值将作为全局变量，里面承载很多重要信息。

具体看一下context的结构定义：

```rust
pub struct InnerContext {
    builder: CircuitBuilder,
    next_var_id: AtomicUsize,
    pending_eq: Vec<(VarId, VarId)>,
    var_map: HashMap<VarId, Tag>,
}

pub struct CircuitBuilder {
    gates: Vec<Gate>,
    permutation: PermutationBuilder<3>,
}

enum Gate {
    Mul,
    Add,

    // ensures that the circuit can be padded to the appropriate size, which is particularly important for FFT operations required in polynomial commitment schemes like PLONK
    Dummy,
}

pub struct PermutationBuilder<const C: usize> {
    /// stores the constraints(equal) between tags
    constrains: HashMap<Tag, Vec<Tag>>,
    /// the number of rows in the matrix(contraints number)
    rows: usize,
}
```

其中`builder`用于定义和承载gates信息和电路的拷贝约束关系。

为了实现拷贝约束，我们需要对每一个约束行的input left，input right和output都做唯一标识，所以这里定义一个`next_var_id`，它是一个自增的整数，每有一个新的w_a, w_b, w_c输入，就对该值赋一个不同的var_id。

`pending_eq`的作用是有w_a, w_b, w_c有var_id之后就可以去保存equal的约束，在结合`var_map`可以建立var_id和Tag之间的对应映射关系，这两个值将来可以用于permutation constrains的hash map。

接下来具体分析一下compile函数。

`let inputs = [(); I].map(|_| BuildVar::input(&context));`的作用是对输入数据做初始化，因为我们这里输入的是[3,4,5]，所以实际上对left_1, left_2, left_3做初始化并赋值id为0，1，2（I值为3），context使用Rc::clone的方式构造全局变量，方便将来添加innerContext的其他内容。

随后`circuit(inputs);`语句就是实际执行之前的run函数。此时的circuit定义为`BuildVar`，而`BuildVar`实现了Add和Mul方法，也就是说，add和Mul运算不会走默认方法，而是走`BuildVar`定义的方式。

```rust
impl Mul for BuildVar {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.binary_operation(&rhs, GateOperation::Mul)
    }
}

impl Add for BuildVar {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.binary_operation(&rhs, GateOperation::Sum)
    }
}

fn binary_operation(&self, rhs: &Self, operation: GateOperation) -> Self {
	let Self { context, id } = &self;
	let right_id = &rhs.id;
	let ids = [(id, 0_usize), (right_id, 1)];

	let j = context.add_gate(operation.build());
	let output_tag = Tag { i: 2, j };
	let id = context.new_id();
	{
		context.add_var(id, output_tag);
	}
	ids.into_iter()
		.map(|(id, i)| (id, context.clone().get_var(&id), i))
		.for_each(|(id, tag, i)| {
			let context = context.clone();
			match tag {
				Some(_) => {
					let new_id = context.new_id();
					context.add_var(new_id, Tag { i, j });
					context.add_eq(*id, new_id);
				}
				None => {
					context.add_var(*id, Tag { i, j });
				}
			};
		});
	Self {
		id,
		context: context.clone(),
	}
}
```

`binary_operation`的主要作用是给context赋值，确定排列关系。其中Tag中的i表示的是列而j表示的是行。下表是对开头程序执行`binary_operation`运算之后的排列关系，其中颜色相同的部分表示两者实际上是相同的。位置关于通过Tag标识，括号里面的内容是var_id。记录这个排列位置关系和相等性约束用于将来的拷贝排列。

这个设计非常巧妙，通过`get_var(&id)`来查询var_id，因为id具有唯一性，所以如果查询到就说明该值之前已经加入到过context了。遍历的`ids.into_iter()`是来自新的行的输入，而竟然能够从var_id中找到和之前一致的var_id，就说明新的输入和原来的context的值有完全相等的对应关系。所以就把该相等对应关系通过`add_eq`加入到permutation的排列数据结构中。同时对新输入赋值一个新的var_id，之所以要赋值新的var_id就是要保证var_id和Tag有一一对应关系。

$$
\begin{array}{c|c|c|c|}
\text{Operation} & \text{Left} & \text{Right} & \text{Output} \\
\hline
\text{mul} & \color{red}\text{left1 (0)} & \color{red}\text{right1 (4)} & {\color{cyan}\text{out1 (3)}} \\
\text{mul} & \color{orange}\text{left2 (1)} & \color{orange}\text{right2 (6)} & {\color{blue}\text{out2 (5)}} \\
\text{mul} & \color{green}\text{left3 (2)} & \color{green}\text{right3 (8)} & {\color{purple}\text{out3 (7)}} \\
\text{add} & \color{cyan}\text{left4 (10)} & \color{blue}\text{right4 (11)} & {\color{purple}\text{out4 (9)}} \\
\end{array}
$$

随后是执行finish运算。

```rust
let (gates, mut permutation) = context.finish();

fn finish(self) -> (Vec<Gate>, PermutationBuilder<3>) {
	let pending_eq = {
		let mut inner = self.inner.lock().unwrap();
		std::mem::take(&mut inner.pending_eq)
	};
	for (left, right) in pending_eq {
		self.add_eq(left, right);
	}
	let mut inner = Rc::try_unwrap(self.inner).unwrap().into_inner().unwrap();
	assert!(inner.pending_eq.is_empty());
	inner.builder.fill();
	let InnerContext {
		builder: CircuitBuilder {
			gates, permutation, ..
		},
		..
	} = inner;
	(gates, permutation)
}

fn fill(&mut self) {
	let rows = self.gates.len();
	let size = repeat(())
		.scan(2_usize, |state, _| {
			let old = *state;
			*state = old * 2;
			Some(old)
		})
		.find(|size| *size >= rows + 3)
		.unwrap();
	self.gates.resize(size, Gate::Dummy);
    }
```

finish函数一方面是如果`pending_eq`有遗漏的，就将其加入到permutation中。另一方面是执行`fill`，对gates也就是行将其数值扩展到 $2^n$ 这个级别，方便后面做FFT运算。补的新的gate用Dummy标识，表示实际上是不参与约束运算，只是占位作用。

继续回到compile函数。这段主要功能是根据每个gate生成对应的行的0和1项，也就是标识该约束运算是乘法门还是加法门。然后将其导入polys，polys表示的是`[[q_L], [q_R], [q_O], [q_M], [q_C]]`也就是 q_L，q_R等列向量，方便将来做该约束运算：

$$
q_L \circ w_a + q_R \circ w_b + q_M\circ(w_a\cdot w_b) + q_C - q_O\circ w_c = 0
$$

```rust
// rows is 2^n
let rows = gates.len();
let domain = <GeneralEvaluationDomain<Fr>>::new(rows).unwrap();
let srs = Srs::random(domain.size());
let mut polys = [(); 5].map(|_| <Vec<Fr>>::with_capacity(rows));
gates.into_iter().for_each(|gate| {
	let row = gate.to_row();
	polys
		.iter_mut()
		.zip(row.into_iter())
		.for_each(|(col, value)| col.push(value));
});
let permutation = permutation.build(rows);
let permutation = permutation.compile();
```

build函数的主要作用是做相等性约束的位置交换，以生成新的排列。该函数设计使用了[union-find](https://github.com/jiajunhua/labuladong-fucking-algorithm/blob/master/%E7%AE%97%E6%B3%95%E6%80%9D%E7%BB%B4%E7%B3%BB%E5%88%97/UnionFind%E7%AE%97%E6%B3%95%E8%AF%A6%E8%A7%A3.md)的思想，将排列交换转换成图的连通性问题，把有相等性关系的点连成一棵树。不过该算法只涉及union部分而无关find。

假设一个排列里有一些相等性节点，要把这些节点串成树总要有一个规范（顺序）。比如说已经有两个节点组成一棵树，这个时候找到第三个相等节点，就要第三个节点加到之前的树上面。所以该函数使用了`sizes`用于记录相等节点数到底有多少，确定新加节点的顺序。在初始化的时候使用的1来做初始化也就意味着视作每一个节点都是完全独立不相等的。然后每找到一个相等的节点，就将其加到树长度最大的节点下面，`sizes[aux[left]] += sizes[aux[right]];`表示树的长度扩展。具体树的映射，也就是树的子节点要指向父节点通过`aux`实现。`if sizes[aux[left]] < sizes[aux[right]]`是来保证小的树始终添加到大的树上面来保证树的平衡。

从constrains里面获取hashMap，代码后面的left和right全部都具有相等性。

数组除了可以表示存储数值的列表，其实还可以表示一种映射关系。比如说有数组`[1,2,4,3]`就可以表示为`0 -> 1 , 1 -> 2 , 2 -> 4`，数组的位置表示原始数据，而该数组对应位置的值表示映射数据，`aux`的作用就是记录这个映射关系。`let aux_left = aux[left];`中`aux_left`表示的是树的根节点。

`mapping`的作用和`aux`类似，也是储存映射关系。所不同的是mapping是最终的permutation排列输出，所以不会是所有的相等性节点都指向共同的根节点，而是有一个swap的关系。`aux`不受该影响，所有的子节点都指向相同的根节点。为了保证mapping经过swap之后的数据仍旧能够在`aux`中找到共同的根节点（也就是保证mapping的映射图形成循环图），所以有一个loop的循环操作。

```rust
pub fn build(&mut self, size: usize) -> Permutation<C> {
	let len = size * C;
	let mut mapping = (0..len).collect::<Vec<_>>();
	let mut aux = (0..len).collect::<Vec<_>>();
	let mut sizes = std::iter::repeat(1).take(len).collect::<Vec<_>>();
	let constrains = std::mem::take(&mut self.constrains);
	for (left, rights) in constrains.into_iter() {
		let mut left = left.to_index(&size);
		for right in rights {
			let mut right = right.to_index(&size);
			if aux[left] == aux[right] {
				continue;
			}
			if sizes[aux[left]] < sizes[aux[right]] {
				swap(&mut left, &mut right);
			}
			sizes[aux[left]] += sizes[aux[right]];
			//step 4
			let mut next = right;
			let aux_left = aux[left];
			loop {
				aux[next] = aux_left;
				next = mapping[next];
				if aux[next] == aux_left {
					break;
				}
			}
			mapping.swap(left, right);
		}
	}
	Permutation { perm: mapping }
}
```

通过build函数生成的拷贝约束之后就可以根据新的permutation构建置换后`[w_a, w_b, w_c]`所对应的新的位置向量。之前permutation生成的新位置向量还是基于自然数自增的（也就是0，1，2，3...）但是这些位置只需要用不同的数来区别表示即可，不一定非要用自然数自增才行。所以在compile函数中，用陪集的方式替换原有的permeation数据，形成新的permutation，此时新的permutation是按列构造的（就是对应的列向量），之所以用陪集替换原有的数据，是为了将来算vanish polynomial时会快一点。

举一个实际用陪集计算的例子，令 $k_1=g^1,k_2=g^2$，计算：

$$
\begin{split}
\vec{id}_a &= (1,\omega,\omega^2,\omega^3) &= [1, 10, 100, 91]\\
\vec{id}_b &= (k_1,k_1\omega,k_1\omega^2,k_1\omega^3) &= [2, 20, 99, 81] \\
\vec{id}_c &= (k_2,k_2\omega,k_2\omega^2,k_2\omega^3) &= [4, 40, 97, 61] \\
\end{split}
$$

原始位置向量 $id_{a}, id_{b}, id_{c}$：


$$
\begin{array}{c|c|c|c|}
i & id_{a,i} & id_{b,i} & id_{c,i} \\
\hline
0 & 1 & 2 & {\color{green}4} \\
1 & {\color{red}10} & {\color{blue}20} & {\color{green}40} \\
2 & 100 & 99 & {\color{red}97} \\
3 & 91 & 81 & {\color{blue}61} \\
\end{array}
$$

置换后的向量 $\sigma_a, \sigma_b, \sigma_c$：

$$
\begin{array}{c|c|c|c|}
i & \sigma_{a,i} & \sigma_{b,i} & \sigma_{c,i} \\
\hline
0 & 1 & 2 & {\color{green}40} \\
1 & {\color{red}97} & {\color{blue}61} & {\color{green}4} \\
2 & 100 & 99 & {\color{red}10} \\
3 & 91 & 81 & {\color{blue}20} \\
\end{array}
$$

代码中的tag表示的是 $id_{?}$ value 表示的是 $\sigma_{?}$ 。通过`let tag = Tag::from_index(index, &rows);`实际指向的是替换后排列的位置，通过执行陪集运算`let value = cosets[tag.i] * roots[tag.j];`实际上就是将原来位置的陪集数据挪到新排列点上。

经过compile运算后，permutation现在为原始位置向量和置换向量的陪集表示。

```rust
pub fn compile(self) -> CompiledPermutation<C> {
	assert_eq!(self.perm.len() % C, 0);
	let rows = self.perm.len() / C;
	let cols = self.perm.chunks(rows);
	let cosets = Self::cosets(rows);
	let domain = <GeneralEvaluationDomain<Fr>>::new(rows).unwrap();
	let roots = domain.elements().collect::<Vec<_>>();
	let perm = cols.enumerate().map(|(i, col)| {
		let coefficients = col
			.iter()
			.enumerate()
			.map(|(j, index)| {
				let tag = Tag::from_index(index, &rows);
				let value = cosets[tag.i] * roots[tag.j];
				let tag = cosets[i] * roots[j];
				(tag, value)
			})
			.collect();
		coefficients
	});
	let mut cols: [Vec<(Fr, Fr)>; C] = [0_u8; C].map(|_| Default::default());
	for (i, col) in perm.enumerate() {
		cols[i] = col;
	}
	CompiledPermutation { cols, cosets, rows }
}
```

继续回到最初的compile函数中。后续的代码是做一些基本处理工作。`let polys = polys.map(|evals| {`是对`[[q_L], [q_R], [q_O], [q_M], [q_C]]`生成多项式表示以及对每一列生成对应的commitment。`let commitments = polys`随后分割出来commitment数组，后续对一些项做赋值。

```rust
let scheme = KzgScheme::new(&srs);
let polys = polys.map(|evals| {
	let poly = Evaluations::from_vec_and_domain(evals, domain).interpolate();
	let commitment = scheme.commit(&poly);
	(poly, commitment)
});
let commitments = polys
	.iter()
	.map(|(_, commitment)| commitment.clone())
	.collect::<Vec<_>>();
let polys = polys.map(|(poly, _)| poly);

let [q_l, q_r, q_o, q_m, q_c] = polys;
let gate_constrains = GateConstrains {
	q_l,
	q_r,
	q_o,
	q_m,
	q_c,
	fixed_commitments: commitments.try_into().unwrap(),
};
CompiledCircuit {
	gate_constrains,
	copy_constrains: permutation,
	srs,
	domain,
	circuit_definition: PhantomData,
	rows,
}
```

## Prove

将输入函数转换为电路的compile环节告一段落接下来就是prove和verify环节。首先看prove整体代码。

```rust
pub fn prove(&self, inputs: [impl Into<Fr>; I], public_inputs: Vec<impl Into<Fr>>) -> Proof {
	let inputs = inputs.map(Into::into);
	let public_inputs = public_inputs
		.into_iter()
		.map(Into::into)
		.collect::<Vec<_>>();

	let advice: [Vec<Fr>; 3] = Default::default();
	let advice = Rc::new(Mutex::new(advice));
	let inputs = inputs.map(|input| ComputeVar {
		value: input,
		advice_values: advice.clone(),
	});

	C::run(inputs);
	let advice = Rc::try_unwrap(advice).unwrap().into_inner().unwrap();
	let mut rng = rand::thread_rng();
	let advice = advice
		.map(|mut col| {
			col.resize(self.rows - 3, Fr::zero());
			let r = [(); 3].map(|_| Fr::rand(&mut rng));
			col.extend_from_slice(&r);
			col
		})
		.map(|col| Evaluations::from_vec_and_domain(col, self.domain).interpolate());

	let mut public_inputs = public_inputs.to_vec();
	public_inputs.resize(self.rows, Fr::zero());

	let proof = prove(&self, advice, public_inputs);
	proof
}
```

prove函数首先对inputs数据和advice做初始化，advice是实际的`[w_a, w_b, w_c]`数组，`inputs.value`中存储的是input对应数据。

初始化完成之后执行`C::run(inputs);`，此时执行的不再是BuildVar而是ComputeVar。其作用就是advice的 $W$ 矩阵。

```rust
fn binary_operation(&self, rhs: &Self, operation: GateOperation) -> Self {
	let left = self.value;
	let right = rhs.value;
	let value = operation.compute(left, right);
	{
		let mut advice = self.advice_values.lock().unwrap();
		advice
			.iter_mut()
			.zip([left, right, value].into_iter())
			.for_each(|(col, value)| col.push(value));
	}
	Self {
		value,
		advice_values: self.advice_values.clone(),
	}
}
```

有了advice的 $W$ 矩阵之后，对advice数据做进一步处理。下述代码首先删除每列的最后3行dummy项，随后添加3个随机项再生成插值多项式。这样做的意义是进一步提供 $W$ 矩阵或者说由 $W$ 矩阵生成的插值多项式安全性，防止秘密信息被破解。

```rust
let advice = advice
	.map(|mut col| {
		col.resize(self.rows - 3, Fr::zero());
		let r = [(); 3].map(|_| Fr::rand(&mut rng));
		col.extend_from_slice(&r);
		col
	})
	.map(|col| Evaluations::from_vec_and_domain(col, self.domain).interpolate());
```

现在advice已经变成 $W$ 矩阵相关的3个插值多项式。随后对`public_inputs`做padding，接下来进入到prove函数。

```rust
fn prove<const I: usize, C: CircuitDescription<I>>(
    circuit: &CompiledCircuit<I, C>,
    advice: [Poly; 3],
    public_inputs: Vec<Fr>,
) -> Proof {
    let scheme = KzgScheme::new(&circuit.srs);
    let domain = &circuit.domain;
    let w = domain.element(1);

    let public_inputs_poly =
        Evaluations::from_vec_and_domain(public_inputs.clone(), *domain).interpolate();
    let commitments = {
        let [a, b, c] = &advice;
        round1(&a, &b, &c, &scheme)
    };
    let challenge_generator = ChallengeGenerator::with_digest(&commitments);
    let [beta, gamma] = challenge_generator.generate_challenges();
    let values = advice
        .clone()
        .map(|e| e.evaluate_over_domain(*domain).evals.to_vec());

    let (acc_poly, acc_commitment, acc_poly_w) = {
        let domain = domain;
        let mut evals = circuit.copy_constrains.prove(&values, beta, gamma);
        evals.pop();
        let acc_shifted = {
            let mut evals_shifted = evals.clone();
            evals_shifted.rotate_left(1);
            let evals = Evaluations::from_vec_and_domain(evals_shifted, *domain);
            evals.interpolate()
        };
        let acc = Evaluations::from_vec_and_domain(evals, domain.clone());
        let acc = acc.interpolate();
        let commitment = scheme.commit(&acc);
        (acc, commitment, acc_shifted)
    };
    let [a, b, c] = advice;
    let mut challenge_generator = ChallengeGenerator::with_digest(&commitments);
    challenge_generator.digest(&acc_commitment);

    let [alpha, evaluation_point] = challenge_generator.generate_challenges();
    let proof = {
        let public_eval = public_inputs_poly.evaluate(&evaluation_point);
        let quotient = quotient_polynomial(
            circuit,
            [&a, &b, &c],
            (&acc_poly, &acc_poly_w),
            [alpha, beta, gamma],
            public_inputs_poly,
        );
        let mut commitments = commitments.into_iter();
        let openings = [a, b, c].map(|poly| {
            let commitment = commitments.next().unwrap();
            let opening = scheme.open(poly, evaluation_point);
            PolyProof {
                commitment,
                opening,
            }
        });
        let advice_evals = openings
            .iter()
            .map(|open| open.opening.1)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let [a, b, c] = openings;
        let z = scheme.open(acc_poly.clone(), evaluation_point);
        let zw = scheme.open(acc_poly.clone(), evaluation_point * w);

        let linearisation = linearisation_poly(
            circuit,
            advice_evals,
            [z, zw].map(|open| open.1),
            acc_poly,
            [alpha, beta, gamma],
            evaluation_point,
            &quotient,
            public_eval,
        );
        let r = scheme.open(linearisation, evaluation_point);
        let permutation = PermutationProof {
            commitment: acc_commitment,
            z,
            zw,
        };
        let t = quotient.commit(&scheme);
        Proof {
            a,
            b,
            c,
            permutation,
            evaluation_point,
            t,
            r,
            public_inputs,
        }
    };
    proof
}
```

该函数首先对advice的3个插值多项式生成对应的commitments。还要根据该commitments生成对应 $\beta$ 和 $\gamma$ 的challenge。

随后对advice多项式在domain上的多个点做evaluate（也就是在 $[w^0, w^1, w^2, ...]$ 做evaluate）。

```rust
let values = advice
        .clone()
        .map(|e| e.evaluate_over_domain(*domain).evals.to_vec());
```

之所以要提前做evaluate是为了计算下式做准备。

$$
\begin{split}
f(X)&=\Big(w_a(X)+\beta\cdot {id_a}(X)+\gamma\Big)\Big(w_b(X)+\beta\cdot {id_b}(X)+\gamma\Big)\Big(w_c(X)+\beta\cdot {id_c}(X)+\gamma\Big)\\
g(X)&=\Big(w_a(X)+\beta\cdot {\sigma_a}(X)+\gamma\Big)\Big(w_b(X)+\beta\cdot {\sigma_b}(X)+\gamma\Big)\Big(w_c(X)+\beta\cdot {\sigma_c}(X)+\gamma\Big)\\
\end{split}
$$

式中 $W_?(X)$ 都是domain点对应的evaluation，可以提前计算。$id_?(X)$ 对应的是`tag`值之前在compile阶段已经计算得出， $\sigma_?(X)$ 对应的是`value`在compile阶段也计算过了。下述函数就是具体构造该acc的过程。

```rust
pub fn prove(&self, values: &[Vec<Fr>; C], beta: Fr, gamma: Fr) -> Vec<Fr> {
	let cosets = self.cosets;
	// tag(origianl), value(copy permuted)
	let perms = &self.cols;
	assert_eq!(cosets.len(), C);
	let acc = ColIterator::new(values.clone(), perms.clone());
	let acc = acc.scan(Fr::one(), |state, vals| {
		let mut num = Fr::one();
		let mut den = Fr::one();
		let row_val = vals.into_iter().fold(Fr::one(), |state, val| {
			// cell_val -> w_(x)
			let (cell_val, (tag, value)) = val;
			let numerator = cell_val + beta * tag + gamma;
			num *= numerator;
			let denominator = cell_val + beta * value + gamma;
			den *= denominator;
			let result = numerator / denominator;
			state * result
		});

		*state *= row_val;
		Some(*state)
	});
	let iter_one = std::iter::repeat(Fr::one()).take(1);
	let acc = iter_one.clone().chain(acc).collect();
	acc
}

impl<const C: usize> Iterator for ColIterator<C> {
    type Item = Vec<(Fr, (Fr, Fr))>;

    fn next(&mut self) -> Option<Self::Item> {
        let i1 = self.0.iter_mut();
        let i2 = self.1.iter_mut();
        i1.zip(i2)
            .map(|(val, perm)| {
                let val = val.next()?;
                let perm = perm.next()?;
                Some((val, perm))
            })
            .collect()
    }
}
```

该函数首先对acc做new处理之后转换成迭代器。这样可以重写next方法来控制迭代器的行为。将按列迭代，转换成 $W_?(X)$ 的值以及permutation重新排列的值在一起按行迭代。这样在`acc.scan(Fr::one(), |state, vals|`的每一次运算实际上都是在计算 $\frac{f(X)}{g(X)}$ 的累乘。

将 $\frac{f(X)}{g(X)}$ 第一项的值设置为1，后面最后一项累乘的结果应该也是1。刚好和第一项的值重合，可以把最后一项删掉，所以在prove函数中有`evals.pop();`。`let mut evals = circuit.copy_constrains.prove(&values, beta, gamma);` evals是acc在domain上的y值组成的数组。

之前计算已经得到了acc，acc(w*\x)  就只是最后一项为1，所以只需要`evals_shifted.rotate_left(1);`就可以得到acc(w*\x)。

$$
\begin{split}
z_0 &= 1 \\
z_{i+1} &= z_i\cdot \frac{f_i(X)}{g_i(X)}
\end{split}
$$

之前的计算已经得到acc，acc(w*\x) 的y值，那么很容易就可以插值出对应的多项式。用`acc`和`acc_shift`表示。对acc多项式可以计算一下对应的commitment，`let commitment = scheme.commit(&acc);`。

把acc的commitment添加到challenge数组里面，现在W矩阵的commitment和acc的commitment一起组成新的challenge所需的commitment。用这个新的challenge生成 $\alpha$ 和 $\zeta$ ，其中 $\zeta$ 命名为`evaluation_point`，将会作为整体多项式打开的评估点。`let [alpha, evaluation_point] = challenge_generator.generate_challenges();`

public多项式在 $\zeta$ 点打开。`let public_eval = public_inputs_poly.evaluate(&evaluation_point);`

接下来计算商多项式。

```rust
fn quotient_polynomial<const I: usize, C: CircuitDescription<I>>(
    circuit: &CompiledCircuit<I, C>,
    advice: [&Poly; 3],
    // Z and Zw
    acc: (&Poly, &Poly),
    // challenges alpha, beta, gamma
    challenges: [Fr; 3],
    public_inputs: Poly,
) -> SlicedPoly<3> {
    let domain = &circuit.domain;
    let w = domain.element(1);
    let gates = &circuit.gate_constrains;
    let permutation = &circuit.copy_constrains;
    let GateConstrains {
        q_l,
        q_r,
        q_o,
        q_m,
        q_c,
        ..
    } = gates;
    let [a, b, c] = advice;
    let CompiledPermutation { cols, cosets, .. } = permutation;
    let [alpha, beta, gamma] = challenges;

    let line1 = &(&(q_l.naive_mul(a) + q_r.naive_mul(b)) - &(q_o.naive_mul(c))
        + q_m.naive_mul(a).naive_mul(b))
        + q_c
        + public_inputs;
    vanishes(&line1, *domain);

    let line2 = [a, b, c]
        .iter()
        .zip(cosets)
        .map(|(advice, coset)| {
            let rhs = DensePolynomial::from_coefficients_vec(vec![gamma, *coset * beta]);
            *advice + &rhs
        })
        .reduce(|one, other| one.naive_mul(&other))
        .unwrap();
    let line2_eval = line2.evaluate(&w);
    let line2 = line2.naive_mul(acc.0);
    let permutations = cols.clone().map(|col| {
        let eval =
            Evaluations::from_vec_and_domain(col.clone().iter().map(|e| e.1).collect(), *domain);
        eval.interpolate()
    });
    let line3 = [a, b, c]
        .iter()
        .zip(permutations)
        .map(|(advice, permutation)| {
            let gamma = DensePolynomial::from_coefficients_vec(vec![gamma]);
            let perm = permutation.mul(beta);
            *advice + &perm + gamma
        })
        .reduce(|one, other| one.naive_mul(&other))
        .unwrap();
    let line3_eval = line3.evaluate(&w);
    assert_eq!(
        line2_eval * acc.0.evaluate(&w) - line3_eval * acc.0.evaluate(&w.square()),
        Fr::zero()
    );
    let line3 = line3.naive_mul(acc.1);
    let line4 = {
        let l0 = l0_poly(*domain);
        let mut acc = acc.0.clone();
        acc.coeffs[0] -= Fr::from(1);
        acc.naive_mul(&l0)
    };
    vanishes(&line4, *domain);
    let combination_element = [Fr::one(), alpha, alpha, alpha.square()];

    let constrains = [line1, line2, -line3, line4];
    let target = constrains
        .into_iter()
        .zip(combination_element.into_iter())
        .map(|(constrain, elem)| constrain.mul(elem))
        .reduce(Add::add)
        .unwrap();

    //vanishes(&target, *domain);
    let target = target.divide_by_vanishing_poly(*domain).unwrap();
    SlicedPoly::from_poly(target.0, domain.size())
}
```

在该函数中，首先做一些基本的数据处理准备工作后计算line1的多项式表示形式。

$$
line1 = q_L \circ w_a + q_R \circ w_b + q_M\circ(w_a\cdot w_b) + q_C + public\_inputs - q_O\circ w_c
$$

随后在`vanishes(&line1, *domain);`验证该多项式表示是否能根据vanish polynomial分解成商多项式而reminder多项式为0。

line2计算的是`*advice + &rhs`也就是

$$
w_i + \gamma + \beta \cdot coset_i \cdot x
$$

随后再连乘起来即

$$
(w_a + \gamma + \beta \cdot coset_1 \cdot x)(w_b + \gamma + \beta \cdot coset_2 \cdot x)(w_c + \gamma + \beta \cdot coset_3 \cdot x)z(x)
$$

接着对permutation做处理，针对 $\sigma$ 向量生成对应3个插值多项式。

line3是

$$
(w_a + \gamma + \beta \cdot \sigma_a(x))(w_b + \gamma + \beta \cdot \sigma_b(x))(w_c + \gamma + \beta \cdot \sigma_c(x))z(wx)
$$

这段代码来保证运算确实有相等性

```rust
assert_eq!(
	line2_eval * acc.0.evaluate(&w) - line3_eval * acc.0.evaluate(&w.square()),
	Fr::zero()
);
```

之前有

$$
\begin{split}
z_0 &= 1 \\
z_{i+1} &= z_i\cdot \frac{f_i(X)}{g_i(X)}
\end{split}
$$

上述代码翻译成公式就是

$$
f(X)z(X) - g(X)z(wX)=0
$$

随后是line4。Lin4是要构造

$$
L_0(X)(z(X)-1)
$$

最终构造的是

$$
\begin{split}
q_L \circ w_a + q_R \circ w_b + q_M\circ(w_a\cdot w_b) + q_C + public\_inputs - q_O\circ w_c + \\
 \alpha(f(X)z(X) - g(X)z(wX)) + a^2(L_0(X)(z(X)-1))
\end{split}
$$

然后除以消失多项式，可以算出对应的商多项式t。t会拆分成 $t_{low}(x)$ $t_{mid}(x)$ $t_{high}(x)$ 

继续回到prove函数。

因为verifier要自己构造对应的challenge点，所以提前要把一些commitment做准备工作传过去，比如W矩阵。

```rust
let openings = [a, b, c].map(|poly| {
	let commitment = commitments.next().unwrap();
	let opening = scheme.open(poly, evaluation_point);
	PolyProof {
		commitment,
		opening,
	}
});

let [a, b, c] = openings;
```

表示在 $\zeta$ 点打开的值还有对应的commitment。

之前的构造函数有多个地方是多项式相乘的形式，可以在评估点提前将部分多项式提前打开，来简化计算。所以可以提前计算 $[ w_a, w_b, w_c ]$ 在 $\zeta$ 点的评估点 $[ \bar{w}_a, \bar{w}_b, \bar{w}_c ]$

与此同时，还对`z(x)`和`z(w*x)` 以及public。

随后进入`linearisation_poly`函数。计算的结果是

$$
\begin{split}
linear = q_L \circ \bar{w}_a + q_R \circ \bar{w}_b + q_M\circ(\bar{w}_a\cdot \bar{w}_b) + q_C + \bar{public}\_inputs - q_O\circ \bar{w}_c + \\
\alpha(f(\zeta){z}(X) - g(\zeta){z}(wX)) + a^2(L_0(\zeta)(\bar{z}(X)-1)) - t \cdot z_H(\zeta)
\end{split}
$$

其中 $f(\zeta)=(\bar{w}_a + \gamma + \beta \cdot coset_1 \cdot \zeta)...$，$g(\zeta)$ 同理(不过代码实际不是在 $g(\zeta)$ 完全打开，而是在a和b打开，c保留成多项式的形式，同时在 $z(w\zeta)$ 打开)。

随后在评估点 $\zeta$ 将linear值打开（将计算结果命名为r）。以及一些其他的评估点，一起作为proof发送给verifier。`let t = quotient.commit(&scheme);`是对之前的 $t_{low}(x)$ $t_{mid}(x)$ $t_{high}(x)$ 每一个都生成一个commitment。

## Verify

verify的代码和proof的原理十分类似，首先和prove的方式一致的构造出一些challenge点，这里用到commitment也是为什么要把commitment传下来的原因。

函数之前做一些数据准备工作，接下来进入`let (advice, acc) = match verify_openings(proof, scheme, domain.element(1)) {`，在该函数（`verify_openings`）中，

```rust
let valid = advice.iter().all(|proof| {
	let PolyProof {
		commitment,
		opening,
	} = proof;
	scheme.verify(commitment, opening, evaluation_point)
});
```

verify的作用是来验证opening P(z) = y，就是实际构造一个多项式承诺的验证，确保承诺的数据确实是对应的数据而不是修改成的别的值，opening里面有两个数据，一个是f(s)，另外一个是h(s)，所以是可以构造出来一个多项式承诺的验证的。该段代码的作用是验证W矩阵的commitment确实是来自proof多项式计算的值，而不是另外专门生成可以欺骗verifier的。随后对z(x)和z(xw)也做同样的事情，最后返回在open点的评估值，`(advice, acc)`advice对应a，b，c。acc对应z(x)和z(xw)。

随后进入`linearisation_commitment`函数，该函数的作用是构造linear函数的commitment，之前已经proof有了评估点 $\zeta$ 将linear值打开（将计算结果命名为r），现在有了commitment之后就做多项式承诺的验证工作。
`let open_valid = scheme.verify(&r, &r_opening, eval_point);`来验证绑定关系是成立的。具体来看一下代码

代码构造方式同proof环节的`linearisation_poly`是一致的，所不同的是最后proof回使用 $\zeta$ 作为评估点生成 $f(\zeta)$ ，此处会使用srs作为评估点，生成commitment。为什么这个式子计算的时候还是绑定诸如 $\bar{w}_a$ 这些点明明是在 $\zeta$ 评估的，而不是在srs，把它视为一个点而不是参与运算的多项式会更好一点，这样的点用于计算可以起到多项式绑定的效果。

