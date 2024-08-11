memory instruction

In the context of the `read_mem` and `write_mem` instructions, the `p` referred to is not a pointer of the operational stack itself, but rather the value at the top of the stack which acts as a pointer to a RAM address.

_ means stack bottom

# VM

首先来看一下vm是如何构建的，指令介绍来自[文档](https://triton-vm.org/spec/instructions.html)

从`vm.rs`开始看起，首先要将一系列字符串或者字符级别的数据结构转换成program实际可执行的内容，一开始要做的是program的初始化。

```rust
pub struct Program {
    pub instructions: Vec<Instruction>,
    address_to_label: HashMap<u64, String>,
    breakpoints: Vec<bool>,
    type_hints: HashMap<u64, Vec<TypeHint>>,
}

impl Program {
    pub fn new(labelled_instructions: &[LabelledInstruction]) -> Self {
        let label_to_address = Self::build_label_to_address_map(labelled_instructions);
        let instructions =
            Self::turn_labels_into_addresses(labelled_instructions, &label_to_address);
        let address_to_label = Self::flip_map(label_to_address);
        let (breakpoints, type_hints) = Self::extract_debug_information(labelled_instructions);

        assert_eq!(instructions.len(), breakpoints.len());
        Program {
            instructions,
            address_to_label,
            breakpoints,
            type_hints,
        }
    }
```

我们以某一个程序为例，看一下他是如何转换的。跳过宏运算暂时不去考虑他。这里对语法稍微做个补充介绍，`call`后面对应的可以看作是一个label，只是一个标识，实际上新的程序执行开始位置（需要记一下这个instruction ip将来好用到call时实际执行该程序），也可以看作是一个调用程序。然后在label里面（诸如`terminate`或`loop_cond`）实际记录该子程序要执行什么指令。

```rust
triton_program!(
	read_io 2    // _ a b
	dup 1        // _ a b a
	dup 1        // _ a b a b
	lt           // _ a b b<a
	skiz         // _ a b
		swap 1   // _ d n where n > d

	loop_cond:
	dup 1
	push 0
	eq
	skiz
		call terminate  // _ d n where d != 0
	dup 1               // _ d n d
	dup 1               // _ d n d n
	div_mod             // _ d n[[ ]]q r
	swap 2              // _ d r q n
	pop 2               // _ d r
	swap 1              // _ r d
	call loop_cond

	terminate:
		// _ d n where d == 0
		write_io 1      // _ d
		halt
)

// after macro! output
Program: [Instruction(ReadIo(N2)), Instruction(Dup(ST1)), Instruction(Dup(ST1)), Instruction(Lt), Instruction(Skiz), Instruction(Swap(ST1)), Label("loop_cond"), Instruction(Dup(ST1)), Instruction(Push(BFieldElement(0))), Instruction(Eq), Instruction(Skiz), Instruction(Call("terminate")), Instruction(Dup(ST1)), Instruction(Dup(ST1)), Instruction(DivMod), Instruction(Swap(ST2)), Instruction(Pop(N2)), Instruction(Swap(ST1)), Instruction(Call("loop_cond")), Label("terminate"), Instruction(WriteIo(N1)), Instruction(Halt)]
```

宏运算后的输出结果会进入到new函数里进一步转换成Program的数据结构。

```rust
fn build_label_to_address_map(program: &[LabelledInstruction]) -> HashMap<String, u64> {
	let mut label_map = HashMap::new();
	let mut instruction_pointer = 0;

	for labelled_instruction in program {
		if let LabelledInstruction::Instruction(instruction) = labelled_instruction {
			instruction_pointer += instruction.size() as u64;
			continue;
		}

		let LabelledInstruction::Label(label) = labelled_instruction else {
			continue;
		};
		let Entry::Vacant(new_label_map_entry) = label_map.entry(label.clone()) else {
			panic!("Duplicate label: {label}");
		};
		new_label_map_entry.insert(instruction_pointer);
	}

	label_map
}
```

首先说一下关于hash map和match的语法，`new_label_map_entry`这里虽然没有算是声明新的变量，但是这是一种条件运算，当条件满足的时候可以执行，也就是当条件满足的时候，可以将`new_label_map_entry`当成是`label_map`。同理`label_map.entry(label.clone())`中里面的`label`也是之前let 设置的`label`。

还是之前的汇编例子，该程序执行完输出是

```shell
label_to_address: {"terminate": 31, "loop_cond": 10}
```

作用是用来记录子程序在instruction整体列表里面的开始ip位置，之所以需要这个是用户在编写汇编的时候是不去算`call subfunction` 这个function所在的ip位置的，所以需要由程序运算自己生成一下。但是从`build_label_to_address_map`中也可以看到这个label必须只能是第一个label，什么意思？就是一段汇编，不能出现两个`terminate`标识，因为这样的话，如果`call terminate`就会不清楚到底要call哪一个了。这在程序中的体现是

```rust
let Entry::Vacant(new_label_map_entry) = label_map.entry(label.clone()) else {
	panic!("Duplicate label: {label}");
};
```

如果label已经在hash map里了，那么就不会是`Vacant`而是`Occupied`此时会进入到else报错。

总的来说该函数的作用是确定label在instruction中ip的位置。

回到new函数继续往后看，接下来进入到`turn_labels_into_addresses`函数，该函数的作用是将之前计算好的`label_to_address`值重新填回到instruction里面的call里面去，该函数的输出会是

```shell
instructions: [ReadIo(N2), ReadIo(N2), Dup(ST1), Dup(ST1), Dup(ST1), Dup(ST1), Lt, Skiz, Swap(ST1), Swap(ST1), Dup(ST1), Dup(ST1), Push(BFieldElement(0)), Push(BFieldElement(0)), Eq, Skiz, Call(BFieldElement(133143986145)), Call(BFieldElement(133143986145)), Dup(ST1), Dup(ST1), Dup(ST1), Dup(ST1), DivMod, Swap(ST2), Swap(ST2), Pop(N2), Pop(N2), Swap(ST1), Swap(ST1), Call(BFieldElement(42949672950)), Call(BFieldElement(42949672950)), WriteIo(N1), WriteIo(N1), Halt]
```

首先说一下call设置的值有一点奇怪，为什么不是10或者31，而是一个比较大的值？

这是因为将输入的10或者31使用了[Montgomery representation](https://en.wikipedia.org/wiki/Montgomery_modular_multiplication) 具体我还没细看，据说是这样表示后会让某些计算更便捷。（具体理论依据来自[github discussion](https://github.com/facebook/winterfell/pull/101) and [paper](https://eprint.iacr.org/2022/274.pdf)）具体转换方式计算如下，大致是做了一个乘法和求模运算。

```rust
label_map: {"loop_cond": 10, "terminate": 31}
a: 31
maybe_address: Some(BFieldElement(133143986145))

const MODULUS: u64 = (1u64 << 64) - (1u64 << 32) + 1; // This is 2^64 - 2^32 + 1
let value = 31;
let montgomery_representation = (value * CONSTANT) % MODULUS; // `CONSTANT` depends on your Montgomery setup
```

接下来具体看一下代码，首先看一下这个`turn_labels_into_addresses`函数的子函数`turn_label_to_address_for_instruction`说一下里面的`map_call_address`的闭包语法。

`map_call_address(|label| Self::address_for_label(label, label_map))`可以看到里面使用了 一个闭包，也就是说该函数传入的参数是一个函数，而该函数又是一个匿名函数，函数的参数是`label`，函数体要执行的内容是`Self::address_for_label(label, label_map)`，所以这里面的`label`只是函数参数而已，并不需要之前设置变量，可以是`label`也可以写成任何其他的值，只是一个代号，这个`label`什么时候赋值呢？要等到函数具体执行的时候才会赋值确定。举个例子

```rust
fn main() {
    let x = 10;

    // Define a closure that takes one parameter `y`
    let closure = |y| {
        // Inside the closure, `x` and `y` are used
        println!("x: {}, y: {}", x, y);
    };

    // Call the closure with a value for `y`
    closure(20); // This will print: "x: 10, y: 20"
}
```

这里`closure`显示的给一个函数名，上述只是一个匿名而已，在closure实际执行的时候会把变量的值赋进去。具体回到`map_call_address`它在实际执行的时候也是会赋值然后计算。当然现在这里`map_call_address`是一个立即执行函数。具体进入到该函数代码中看一下

```rust
pub(crate) fn map_call_address<F, NewDest>(&self, f: F) -> AnInstruction<NewDest>
where
	F: FnOnce(&Dest) -> NewDest,
	NewDest: PartialEq + Default,
{
	match self {
		Pop(x) => Pop(*x),
		Push(x) => Push(*x),
		Divine(x) => Divine(*x),
		Dup(x) => Dup(*x),
		Swap(x) => Swap(*x),
		Halt => Halt,
		Nop => Nop,
		Skiz => Skiz,
		Call(label) => Call(f(label)),
		Return => Return,
```

可以看到f只是在match匹配的时候才执行生效（函数式编程函数延迟执行用意在这里），当匹配到需要`f`的时候，那么就执行外部的匿名函数，就是之前说的那个。`label_map`是外部定义好的，通过闭包的方式直接塞进来了。`label`是`call`的时候赋值进去的，也就是我们之前看到的`Instruction(Call("terminate"))`，这里面的`terminate`就是赋予`label`的值，在这里`f(label)`把值塞到匿名函数里了。

回到`turn_labels_into_addresses`这个函数，`.flat_map(|inst| vec![inst; inst.size()])`这个内容是根据

```rust
pub fn size(&self) -> usize {
        match self {
            Pop(_) | Push(_) => 2,
            Divine(_) => 2,
            Dup(_) | Swap(_) => 2,
            Call(_) => 2,
            ReadMem(_) | WriteMem(_) => 2,
            ReadIo(_) | WriteIo(_) => 2,
            _ => 1,
        }
    }
```

对指令做一个复制，然后flat。总的来说`turn_labels_into_addresses`这个函数是对所有的指令根据size做一次复制操作，然后过滤掉之前的label，以及对call的ip值做一次赋值操作。

回到之前的new函数，继续向后看代码，`address_to_label`就是把之前的`label_to_address`的key value反过来，即`address_to_label: {10: "loop_cond", 31: "terminate"}`。

`breakpoints, type_hints`来自debug，暂时不知道有什么用。

把`program.new`函数分析完了，现在根据指令生成了program，接下来进入到`program.run`函数。

首先进入到`VMState::new`。VMState存储整个vm的全部状态信息，包括程序指令，指令指针，输入输出，堆栈内存等等所以的状态信息。VMState输入信息接受program, pubic input 和 secret input。前两个好理解，我们来看一下secret input。

```rust
/// All sources of non-determinism for a program. This includes elements that
/// can be read using instruction `divine`, digests that can be read using
/// instruction `merkle_step`, and an initial state of random-access memory.
pub struct NonDeterminism {
	/// A list of [`BFieldElement`]s the program can read from using instruction `divine`.
    pub individual_tokens: Vec<BFieldElement>,
    /// A list of [`Digest`]s the program can use for instruction `merkle_step`.
    pub digests: Vec<Digest>,
    /// The read-write **random-access memory** allows Triton VM to store arbitrary data.
    pub ram: HashMap<BFieldElement, BFieldElement>,
}
```

接下来看一下new函数是怎么赋值的

```rust
impl VMState {
    /// Create initial `VMState` for a given `program`
    ///
    /// Since `program` is read-only across individual states, and multiple
    /// inner helper functions refer to it, a read-only reference is kept in
    /// the struct.
    pub fn new(
        program: &Program,
        public_input: PublicInput,
        non_determinism: NonDeterminism,
    ) -> Self {
        let program_digest = program.hash();

        Self {
            program: program.instructions.clone(),
            public_input: public_input.individual_tokens.into(),
            public_output: vec![],
            secret_individual_tokens: non_determinism.individual_tokens.into(),
            secret_digests: non_determinism.digests.into(),
            ram: non_determinism.ram,
            ram_calls: vec![],
            op_stack: OpStack::new(program_digest),
            jump_stack: vec![],
            cycle_count: 0,
            instruction_pointer: 0,
            sponge: None,
            halting: false,
        }
    }
```

这里就是把之前的program程序指令以及公开和秘密输入赋初始值，这和正常的vm程序无异，不同的是这里多了一个`let program_digest = program.hash();`以及`OpStack::new(program_digest)`，一般纯执行vm没有这样的设计，该部分的分析会放在 **Program Attestation** 部分。

VMState new过之后就可以run来实际执行指令了。进入到run代码看一下。

```rust
pub fn run(&mut self) -> Result<()> {
        while !self.halting {
            self.step()?;
        }
        Ok(())
    }
```

可以看到run其实就是逐个的step一条一条执行指令。执行指令的时候就需要不停的和堆栈打交道，所以先看一下堆栈的结构设计。

```rust
pub struct OpStack {
    /// The underlying, actual stack. When manually accessing, be aware of reversed indexing:
    /// while `op_stack[0]` is the top of the stack, `op_stack.stack[0]` is the lowest element in
    /// the stack.
    pub stack: Vec<BFieldElement>,

    underflow_io_sequence: Vec<UnderflowIO>,
}
```

可以看到OpStack分成两个部分，第一部分是由16个寄存器组成的stack，第二部分是underflow的memory stack。

接下来进入到`step`函数看一下。

```rust
pub fn step(&mut self) -> Result<Vec<CoProcessorCall>> {
	if self.halting {
		return Err(MachineHalted);
	}

	let current_instruction = self.current_instruction()?;
	let op_stack_delta = current_instruction.op_stack_size_influence();
	if self.op_stack.would_be_too_shallow(op_stack_delta) {
		return Err(OpStackTooShallow);
	}

	self.start_recording_op_stack_calls();
	let mut co_processor_calls = match current_instruction {
		Pop(n) => self.pop(n)?,
		Push(field_element) => self.push(field_element),
		...
```

代码首先做一个停止判断，接下来在`current_instruction`是使用之前VMState定义的`instruction_pointer`来得到当前的instruction。

有的指令会用到堆栈数据操作，比如pop会从堆栈中弹出一些数据，`if self.op_stack.would_be_too_shallow(op_stack_delta)`用来确保比如弹出数据之后堆栈中总的数据量依旧超过16个。（我暂时不知道`stack: Vec<BFieldElement>`是怎么处理数据的，从调试信息来看，初始化时数组元素会有16个而不是我想的那个初始化没有值（当然初始化时确实很多没有值，就用空的`0`赋进去，确实是有16个），然后后期不是始终保持16个，而是值会变化，可能比如会有19个，超过了16个的限制）。

`self.start_recording_op_stack_calls()`是把stack的`underflow_io_sequence`数据清空。根据后面的代码来看`underflow_io_sequence`似乎像是一个临时的memory，用于配合程序指令执行的，在指令执行期间会临时存放一些什么中间数据之类的，不过最终随着一个指令执行完成会把中间数据（如果有且必要）存放给堆栈或者其他需要的地方，然后把`underflow_io_sequence`里面的内容清空，每次指令执行完会清空一次，下一个新的指令要执行前也清空一次，确保指令执行时`underflow_io_sequence`初始化时里面数据时空的。

接下来以pop为例看一下程序指令是如何执行的。

```rust
fn pop(&mut self, n: NumberOfWords) -> Result<Vec<CoProcessorCall>> {
	for _ in 0..n.num_words() {
		self.op_stack.pop()?;
	}

	self.instruction_pointer += 2;
	Ok(vec![])
}

// UnderflowIO struct
pub enum UnderflowIO {
    Read(BFieldElement),
    Write(BFieldElement),
}

// stack method
pub(crate) fn pop(&mut self) -> Result<BFieldElement> {
	self.record_underflow_io(UnderflowIO::Read);
	self.stack.pop().ok_or(OpStackTooShallow)
}

fn record_underflow_io(&mut self, io_type: fn(BFieldElement) -> UnderflowIO) {
	let underflow_io = io_type(self.first_underflow_element());
	self.underflow_io_sequence.push(underflow_io);
}
```

首先再补充一下Rust的语法。enum在Rust中可以不只是单纯的数值枚举，而可以代表更广泛复杂的数据类型。比如这里的`UnderflowIO::Read`可以看成是一种特殊的函数类型（也可以认为是一种映射）`fn(BFieldElement) -> UnderflowIO`。比如`Read`实际上是一种标识符，它最终都是`UnderflowIO`类型，通过这样的标识设计很容易就建立起来`BFieldElement`和`UnderflowIO`的**映射关系**，相当于给原来的`BFieldElement`外面再套了一个马甲改成了`UnderflowIO`类型，很方便的实现了类型转换，通过类型转换之后可以去做自己想做的别的事情。

总的来说，pop的作用就是把stack `Vec<BFieldElement>`中的值pop出去，以及把underflow第一个元素存储到`underflow_io_sequence`中。

回到step函数，`self.stop_recording_op_stack_calls()`清空`underflow_io_sequence`，将`underflow_io_sequence`存储的值排出

```rust
fn stop_recording_op_stack_calls(&mut self) -> Vec<CoProcessorCall> {
	let sequence = self.op_stack.stop_recording_underflow_io_sequence();
	self.underflow_io_sequence_to_co_processor_calls(sequence)
}

fn underflow_io_sequence_to_co_processor_calls(
	&self,
	underflow_io_sequence: Vec<UnderflowIO>,
) -> Vec<CoProcessorCall> {
	let op_stack_table_entries = OpStackTableEntry::from_underflow_io_sequence(
		self.cycle_count,
		self.op_stack.pointer(),
		underflow_io_sequence,
	);
	op_stack_table_entries
		.into_iter()
		.map(OpStackCall)
		.collect()
}
```

用于生成op表，等会分析。。。

最后将op trace返回。当遇到`halt`指令时整个程序终止。

之前的run代码中可以看到`self.step()`并没有接受值，也就是说纯执行vm而言，并不关心trace表。关心trace表的proof后续分析。

# AET

AET是Algebraic Execution Trace，他和后面分析的各种trace table是class和instance的关系，AET是一种总的抽象，各种trace table是具体场景下的实现。

现在将视线转移到`examples/factorial.rs`后续的代码将会从这里为入口展开分析。

进入到`prove_program`这个入口函数，如下指令用于生成stark证明所需要的表，在Trace部分会对表做深入分析。接下来进入到该函数中看一下。

```rust
let (aet, public_output) = program.trace_execution(public_input.clone(), non_determinism)?;

// trace_execution function
pub fn trace_execution(
	&self,
	public_input: PublicInput,
	non_determinism: NonDeterminism,
) -> Result<(AlgebraicExecutionTrace, Vec<BFieldElement>)> {
	profiler!(start "trace execution" ("gen"));
	let state = VMState::new(self, public_input, non_determinism);
	let (aet, terminal_state) = self.trace_execution_of_state(state)?;
	profiler!(stop "trace execution");
	Ok((aet, terminal_state.public_output))
}
```

可以看到`trace_execution`的核心是通过调用`trace_execution_of_state`生成AET，`VMState::new`我们之前已经分析过了，他的作用是根据传入的program然后初始化整个VM state，为后续程序执行以及生成trace做准备。接下来看一下`self.trace_execution_of_state(state)`是如何生成AET的。

```rust
pub fn trace_execution_of_state(
	&self,
	mut state: VMState,
) -> Result<(AlgebraicExecutionTrace, VMState)> {
	let mut aet = AlgebraicExecutionTrace::new(self.clone());
	assert_eq!(self.instructions, state.program);
	assert_eq!(self.len_bwords(), aet.instruction_multiplicities.len());

	while !state.halting {
		if let Err(err) = aet.record_state(&state) {
			return Err(VMError::new(err, state));
		};
		let co_processor_calls = match state.step() {
			Ok(calls) => calls,
			Err(err) => return Err(VMError::new(err, state)),
		};
		for call in co_processor_calls {
			aet.record_co_processor_call(call);
		}
	}

	Ok((aet, state))
}
```

该函数首先生成AET的初始化，因此看一下AET结构定义是什么样的

```rust
pub struct AlgebraicExecutionTrace {
    /// The program that was executed in order to generate the trace.
    pub program: Program,

    /// The number of times each instruction has been executed.
    ///
    /// Each instruction in the `program` has one associated entry in `instruction_multiplicities`,
    /// counting the number of times this specific instruction at that location in the program
    /// memory has been executed.
    pub instruction_multiplicities: Vec<u32>,

    /// Records the state of the processor after each instruction.
    pub processor_trace: Array2<BFieldElement>,

    pub op_stack_underflow_trace: Array2<BFieldElement>,

    pub ram_trace: Array2<BFieldElement>,

    /// The trace of hashing the program whose execution generated this `AlgebraicExecutionTrace`.
    /// The resulting digest
    /// 1. ties a [`Proof`](crate::proof::Proof) to the program it was produced from, and
    /// 1. is accessible to the program being executed.
    pub program_hash_trace: Array2<BFieldElement>,

    /// For the `hash` instruction, the hash trace records the internal state of the Tip5
    /// permutation for each round.
    pub hash_trace: Array2<BFieldElement>,

    /// For the Sponge instructions, i.e., `sponge_init`, `sponge_absorb`,
    /// `sponge_absorb_mem`, and `sponge_squeeze`, the Sponge trace records the
    /// internal state of the Tip5 permutation for each round.
    pub sponge_trace: Array2<BFieldElement>,

    /// The u32 entries hold all pairs of BFieldElements that were written to the U32 Table,
    /// alongside the u32 instruction that was executed at the time. Additionally, it records how
    /// often the instruction was executed with these arguments.
    pub u32_entries: HashMap<U32TableEntry, u64>,

    /// Records how often each entry in the cascade table was looked up.
    pub cascade_table_lookup_multiplicities: HashMap<u16, u64>,

    /// Records how often each entry in the lookup table was looked up.
    pub lookup_table_lookup_multiplicities: [u64; AlgebraicExecutionTrace::LOOKUP_TABLE_HEIGHT],
}
```

AET的理论定义来自[这里](https://triton-vm.org/spec/arithmetization.html?highlight=co-rpocessor#arithmetization)，整个AET可以分成三大块，一块是Processor Table用于表示整个VM流程相关的table。 另外一块是Co-Processor Table，是用来辅助Processor Table的。一些计算比如说hash或者xor这样的计算不是很容易在processor table里做算术化表示，因此设置一些辅助的table来做processor table的补充，比如`hash_trace`以及`u32_entries`都是这样的co-processor table。还有一块是用来保证Memory Consistency的，比如`op_stack_underflow_trace`，`ram_trace`。

看一下new函数，

```rust
pub fn new(program: Program) -> Self {
	let program_len = program.len_bwords();

	let mut aet = Self {
		program,
		instruction_multiplicities: vec![0_u32; program_len],
		processor_trace: Array2::default([0, processor_table::BASE_WIDTH]),
		op_stack_underflow_trace: Array2::default([0, op_stack_table::BASE_WIDTH]),
		ram_trace: Array2::default([0, ram_table::BASE_WIDTH]),
		program_hash_trace: Array2::default([0, hash_table::BASE_WIDTH]),
		hash_trace: Array2::default([0, hash_table::BASE_WIDTH]),
		sponge_trace: Array2::default([0, hash_table::BASE_WIDTH]),
		u32_entries: HashMap::new(),
		cascade_table_lookup_multiplicities: HashMap::new(),
		lookup_table_lookup_multiplicities: [0; Self::LOOKUP_TABLE_HEIGHT],
	};
	aet.fill_program_hash_trace();
	aet
}
```

基本就是做一些各个表的初始化操作。唯一不同的是new里面的`aet.fill_program_hash_trace()`，进入到该函数看一下，该函数的分析在Program Table部分。


#### Program Table

AET的填表是根据程序执行指令然后需要填什么表就填什么表，而不是填完一个表再填一个表，因此很难说某个表的部分的分析就是那个表的内容。尽可能做到一些区分吧，但不绝对。

接之前AET初始化操作，分析一下AET初始化时`fill_program_hash_trace`是怎么填表的。

```rust
/// Hash the program and record the entire Sponge's trace for program attestation.
fn fill_program_hash_trace(&mut self) {
	let padded_program = Self::hash_input_pad_program(&self.program);
	let mut program_sponge = Tip5::init();
	for chunk in padded_program.chunks(Tip5::RATE) {
		program_sponge.state[..Tip5::RATE]
			.iter_mut()
			.zip_eq(chunk)
			.for_each(|(sponge_state_elem, &absorb_elem)| *sponge_state_elem = absorb_elem);
		let hash_trace = program_sponge.trace();
		let trace_addendum = HashTable::trace_to_table_rows(hash_trace);

		self.increase_lookup_multiplicities(hash_trace);
		self.program_hash_trace
			.append(Axis(0), trace_addendum.view())
			.expect("shapes must be identical");
	}

	let instruction_column_index = CI.base_table_index();
	let mut instruction_column = self.program_hash_trace.column_mut(instruction_column_index);
	instruction_column.fill(Instruction::Hash.opcode_b());

	// consistency check
	let program_digest = program_sponge.state[..Digest::LEN].try_into().unwrap();
	let program_digest = Digest::new(program_digest);
	let expected_digest = self.program.hash();
	assert_eq!(expected_digest, program_digest);
}
```

代码首先执行`hash_input_pad_program`，对于hash函数而言，特别是类似sha3这样的有海绵结构的hash函数而言，所有的用于输入的input都是固定结构，这个结构的长度是 $r \cdot s$ ，其中r称之为rate是一个比例（在本文中是chunk size为10），s是倍数，可以是任意倍数，只要输入的input长度可以满足等于上述的 $r \cdot s$ 就可以直接用于hash函数的输入。但是有的时候input长度不满足，这个时候就要用到padding来做填充，填充到满足上述等式为止。填充的规则是首先在input后面填一个1，如果还要填就继续补0。举个例子，比如输入hash的固定长度是512，此时input的size是400，那么就在401位置填1 ，后续位置填0，一直填到512为止。该函数的作用是输入program，然后将program根据opcode将指令转换成数组以及对数组填充。现在有了一个填充好的数组`padded_program`。

上述内容参考自Program Table解释[文档](https://triton-vm.org/spec/program-table.html) 以及Tip5论文

> For [program attestation](https://triton-vm.org/spec/program-attestation.html), the program is [padded](https://triton-vm.org/spec/program-attestation.html#mechanics) and sent to the [Hash Table](https://triton-vm.org/spec/hash-table.html) in chunks of size 10, which is the rate of the [Tip5 hash function](https://eprint.iacr.org/2023/107.pdf). Program padding is one 1 followed by the minimal number of 0’s necessary to make the padded input length a multiple of the rate

接着往后看代码是对Tip5的初始化，首先看一下Tip5的数据结构

```rust
pub struct Tip5 {
	// STATE_SIZE = 16
    pub state: [BFieldElement; STATE_SIZE],
}
```

接下来是`Tip5::init()`在做啥

```rust
pub enum Domain {
    /// The `VariableLength` domain is used for hashing objects that potentially serialize to more
    /// than [`RATE`] number of field elements.
    VariableLength,

    /// The `FixedLength` domain is used for hashing objects that always fit within [RATE] number
    /// of fields elements, e.g. a pair of [Digest].
    FixedLength,
}

impl Tip5 {
    #[inline]
    pub const fn new(domain: Domain) -> Self {
        use Domain::*;

        let mut state = [BFieldElement::ZERO; STATE_SIZE];

        match domain {
            VariableLength => (),
            FixedLength => {
                let mut i = RATE;
                while i < STATE_SIZE {
                    state[i] = BFieldElement::ONE;
                    i += 1;
                }
            }
        }

        Self { state }
    }

impl Sponge for Tip5 {
    const RATE: usize = RATE;

    fn init() -> Self {
        Self::new(Domain::VariableLength)
    }
```

从代码中可以看到`Tip5::init()`是生成了一个新的Tip其中State的值全部是0总共是16个也就是说`program_sponge.state=[0; 16]`。


#### Hash Table

再往后分析代码之前要补充一点理论内容，来自[Hash Table](https://triton-vm.org/spec/hash-table.html)。之前说的`program_sponge.state=[0; 16]`实际上是做`sponge_init`。

>Instruction `sponge_init`
>   1. sets all the hash coprocessor's registers (`state_0` through `state_15`) to 0.

`fill_program_hash_trace`部分随后的代码，也就是从`for chunk in padded_program... let hash_trace = program_sponge.trace();`执行的是`sponge_absorb`也就是

> Instruction `sponge_absorb`
>   1. overwrites the hash coprocessor's rate registers (`state_0` through `state_9`) with the processor's stack registers `state_0` through `state_9`, and
>   2. executes the 5 rounds of the Tip5 permutation.

处理完之后`hash trace`是`[[BFieldElement; 16]; 6]`。

`fill_program_hash_trace`部分随后代码`HashTable::trace_to_table_rows(hash_trace)`是用于生成Hash Table的base columns，具体参见[Hash Table](https://triton-vm.org/spec/hash-table.html)的base columns部分。



# Table Linking

发

# Program Attestation

zkvm有一个问题就是比如prover想要根据某个程序生成证明，verifier来验证该证明，但是verifier怎么知道到底prover是根据哪个程序生成的证明？所以证明和程序要有一个绑定关系，这样才能让verifier信服确实是根据该程序生成的证明。而program attestation就是来提供这样的绑定关系的。

之前的vm部分在`VMState::new()`里有这样一段代码`let program_digest = program.hash();`以及`OpStack::new(program_digest)`就是对整个program程序指令做hash运算然后生成digest，随后放入到op stack中（后续生成表时也会用到），这样就将程序指令和证明进行了绑定。

看一下`OpStack::new`的代码

```rust
pub fn new(program_digest: Digest) -> Self {
	let mut stack = bfe_vec![0; OpStackElement::COUNT];

	let reverse_digest = program_digest.reversed().values();
	stack[..Digest::LEN].copy_from_slice(&reverse_digest);

	Self {
		stack,
		underflow_io_sequence: vec![],
	}
}
```

该代码首先将`stack`初始化为有16个`0`的`vec`，随后将reverse之后的digest放到stack的前5个register中（这里`Digest::LEN`是常数值设置为5）