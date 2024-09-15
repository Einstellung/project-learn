
## 初始化

Cairo语言编译有两条线。一条是Cairo 0，使用的是[cairo-lang](https://github.com/starkware-libs/cairo-lang)，将Cairo代码转换成Cario 0x可以识别的json中间表示。（也是本Cario VM可以识别的版本）

另外一个是后来的Cairo 1x路线，Cairo → Sierra → CASM，然后使用Cario VM解读。

这两者的区别是：
- **Cairo 0.x JSON**: Directly executable by the Cairo VM but lacks the advanced safety checks and optimizations provided by Sierra.
- **Cairo 1.x+ (Sierra and CASM)**: Introduces an intermediate representation (Sierra) that is safer and more optimized. CASM is the final format executed by the Cairo VM in this newer ecosystem.

项目运行时需要按照[规范](https://github.com/lambdaclass/lambdaworks/tree/main/provers/cairo)用docker装一下cairo-lang也就是对应的python编译依赖，然后才可以直接将cairo转换成json，用于后续vm处理。

## Cairo Zero

[How Cairo Works](https://docs.cairo-lang.org/how_cairo_works/index.html)对VM有一个整体介绍。接下来部分是对该部分学习内容的总结。还有一个Cairo vm设计的讲解[视频](https://www.youtube.com/watch?v=4GBgkOT9SCs&ab_channel=StarkNetCC)

#### Field Elements

除法计算是通过inverse的方式去做的，[计算方式](https://github.com/WTFAcademy/WTF-zk/blob/main/06_Division/readme.md)。

#### Program Counter

pc一次最多跳1或2个（根据指令有所不同，不会超）。`jmp`指令跳转的只是program instruction的位置或者说只是更改pc的值。`ret`是return的缩写。

#### Functions

调用function的指令是`call`，比如调用某个function可以写成`call function_name;`其含义相当于`function_name();` ，call还可以用来`call label`。call还可以用来类似`jmp`直接做某个指令的执行（`rel / abs`两种方式），比如`call rel 5;`就是将当前pc跳转向前5执行之后的指令。所不同的是`jmp`之后pc不会回来，而对于call而言，会有一个返回，也就是保留当前call的pc位置，执行完跳转的对应指令之后会返回，然后执行下一条指令。

函数执行时对内存操作有两个指针，分别是`ap`（allocated pointer）和`fp`（frame pointer）。前者是用来操作统一的连续内存，随着程序执行不停变化。后者是程序执行时特地分给局部变量的stack frame。实际没有stack，就是一块线性的内存。只不过`fp`用来存函数的局部变量信息。为什么这么干，原因：

- In Cairo, since memory is allocated in a linear and predictable manner using `ap` (allocation pointer), and the frame pointer `fp` provides a stable reference within each function, it's easier to track and log memory usage over time.
- The absence of automatic memory collection means that the state of memory remains fully deterministic, making it straightforward to generate the trace of each memory access during the program's execution.

为什么要有`fp`呢，因为函数总是有局部变量数据，我可能要去算或者保存局部变量的数据，但是函数运行时有可能调用其他的函数。当call执行完饭回之后我还要能够记住原来的函数的局部变量在哪。因此有了一个`fp`专门用来标识局部变量的位置。

内存是一整块的内存，`ap`和`fp`并不存在给`fp`专门开一个stack frame。实际运行时情况是这样的，比如有一个`fool`函数，在开始执行的时候有`fp=ap`，也就是在`[ap]`开始会存一些局部变量的数据，后面就一直往后存，可能是在`[ap+1]`继续存一些局部变量信息。

假如程序运行一段时间之后发现`fool`函数里面还有`bar`函数，就要执行call指令了。在执行之前需要对一些数据信息做保存，好将来`bar`函数返回之后`fool`函数还能继续执行。首先就是要保存`fool`函数的`fp`位置，之所以要保存在于进入到`bar`函数时`bar`又有自己的`fp`会把原来的`fp`覆盖，所以要保存`fool`的`fp`好将来做恢复。于是有（执行`bar`函数之前）

```python
// before call
[ap]=fp;
[ap+1]=return pc;
ap += 2;

fp=ap;
jmp address;
```

`[ap]=fp;`表示把`fool`的`fp`位置的值赋到当前的`ap`里面，`[ap+1]=return pc;`表示把`fool`的`pc`位置赋值进去，好知道`bar`返回时，`fool`从哪里继续往后执行。`ap += 2;`该语句表示`bar`从`ap+2`开始做内存的使用。

`fp=ap;`表示从这里开始是`bar`的`fp`，`jmp address;`表示跳到`bar`的指令位置。

当bar执行完返回时，做如下计算：

```python
jmp [fp - 1];
fp = [fp - 2];
```

可以看到之前保存的`fp`可以通过`[fp - 1]`知道return pc的位置，`fp = [fp - 2];`可以用来恢复原来的`fp`。

#### Nondeterministic Computation

与之相对的概念是deterministic computation。比如说我要计算 $y=\sqrt x$  对于deterministic computation而言我要输入x然后根据`sqrt`公式一步一步算出y来，而对于non deterministic computation而言，y值不是算出来的，而是猜出来的，我猜一个y值，然后验证 $y^2=x$ 如果成立，就说明自己猜的y值对了。使用non deterministic computation好处是可以减少指令的数量进而减少trace长度。

#### Hints

这个猜的设计，在Cairo中被称之为hints。比如如下代码

```python
[ap] = 25, ap++;
%{
    import math
    memory[ap] = int(math.sqrt(memory[ap - 1]))
%}
[ap - 1] = [ap] * [ap], ap++;
```

y值是提前在外部算出来的（花括号表示的地方），然后在Cario程序里面，验证 $y^2=x$ 是否成立，这样可以节省Cario内部指令开销。

#### Segments

很多的trace都是在程序执行期间才能知道具体结果。这样一来，对内存的操作就不可避免的散落在程序执行trace的很多地方。因此有必要在程序执行完生成trace之后把关于内存的trace提取出来，这样可以对内存单独做处理和约束。于是有了segments。segments发生在原始的trace生成之后，用于生成内存的trace期间，对新生成的memory按照功能模块重新划分，这个过程称之为relocated。实际上segment有很多种，不一定只是memory的，还有可能是pc的等，Cairo zero segment部分以及视频有详细讲解。

一个segment结构样子会是这样的，segment用s:t标识，前者表示哪一个segment，后者表示的是第几个位置。

![Pasted image 20240820174739](https://github.com/user-attachments/assets/34245a7b-e780-453f-bebc-6557d59764d0)


实际上我们只有一块内存，图片中标识的好像是有两块但实际上只是为了表示方便，因为memory设计是`ap`只能不停向前不能后退也不能更改之前的值，那么新加的一个数组添加完之后再想回到原来的`ap`执行之前的数组的值就会占据一段内容，对于上述图片来讲，大概就是

```
[a, b, x, y, (1:4), ...]
```

现在可以对上述的memory按照segment划分重新排列一下

```
seg1:[a, b, (1:4), ...] + seg2: [x, y]
```

重新排列之后的划分大概可以表示成这样

![Pasted image 20240820175856](https://github.com/user-attachments/assets/b96777bc-44e1-4443-8fc3-2df9af2c2dfa)


其中user segment这里就是用户定义的数据或者数据结构，比如之前的那个设置的array `[x, y]`


## Cairo Paper

以下内容是Cairo a Turing complete STARK-friendly CPU architecture论文学习解读。

论文中对递归stark没有详细的介绍，这里有一篇泛泛的[介绍](https://www.chaincatcher.com/article/2078071)

#### Memory

Cairo使用的一种称之为nondeterministic read-only memory的内存模型。内存具体值在程序执行之前就已经被写好了，程序执行期间内存的数据是只读不能修改的。然后读去完数据之后还要验证读取的数据是否和之前声明的值一致。

比如说这样一个例子，在地址20处写入数值7，将来程序执行期间获取数据流程如下：

```python
y = read(address=20)
assert y == 7
...
x = read(address=20)
```

这样的设计模式可以非常好的保证内存数据的连续性，杜绝在这一块内存写一个数据然后又在那一块内存又写一个数据。不需要特别多额外的约束，这样prover只需要5个trace cell就可以构建起整个memory trace。没有内存回收机制，也没有rewrite，因为这样会使得trace变得更复杂，得不偿失。

还有就是之前所说的所有的`ap，fp，pc`等号操作都不是赋值操作，而是声明相等操作。这才满足read only的概念。比如说

```python
[ap] = 10;
[ap] = [ap+1];
[ap+1] = [ap+2] + 5;
```

这里说的不是`[ap]`值赋予10，而是说他应该是10，所以`[ap+2]`的值应该是5。

#### instruction structure

instruction在内存中存储的单元称之为word。每个word的大小是63bit。通常情况下一个instruction包含两部分，指令本身以及operand（如果有的话），指令本身会存储在第一个word，而operand的值会存在第二个（如果更多operand会放在第三个等等）位置。也就是说第二个word会存value，第一个word会存指令。

![Pasted image 20240821164011](https://github.com/user-attachments/assets/b55e2f6b-cc3d-4dac-9027-351e4515289b)


还是要对这个表做详细一点的理解。

`0ff_dst`: designation register的偏移值。
`off_op0`: first operand的偏移值。
`off_op1`: second operand的偏移值。

`dst_reg`: 这是一个标志位，用于表示designation register是`ap`还是`fp`，`ap`用0表示，`fp`用1表示。之后是更新`dst`的值。

```python
# Compute dst
if dst_reg == 0:
    dst = m(ap + off_dst)
else:
    dst = m(fp + off_dst)
```

`op0_reg`: 标识`op0`是`ap`还是`fp`，标识方式同`dst_reg`一致。之后更新`op0`的值。

```go
if op0_reg == 0:
	op0 = m(ap + off_op0)
else:
	op0 = m(fp + off_op0)
```

`op1_src` 用于表示`op1`的计算结果。

```python
# Compute op1 and instruction_size.
switch op1_src:
    case 0:
        instruction_size = 1
        op1 = m(op0 + off_op1)
    case 1:
        instruction_size = 2
        op1 = m(pc + off_op1)
        # If off_op1 == 1, we have op1 = immediate_value.
    case 2:
        instruction_size = 1
        op1 = m(fp + off_op1)
    case 4:
        instruction_size = 1
        op1 = m(ap + off_op1)
    default:
        Undefined Behavior
```

`res_logic`: 用于标识是否需要做逻辑计算。

```python
# Compute res.
if pc_update == 4:
    if res_logic == 0 and opcode == 0 and ap_update != 1:
        res = Unused
    else:
        Undefined Behavior
elif pc_update == 0 or pc_update == 1 or pc_update == 2:
    switch res_logic:
        case 0:
            res = op1
        case 1:
            res = op0 + op1
        case 2:
            res = op0 * op1
        default:
            Undefined Behavior
else:
    Undefined Behavior
```

`ResOp1`表示没有任何计算，结果就是operand 1的值。`ResAdd`表示对`op0`和`op1`做加法计算并返回结果，`ResMul`是乘法计算。可以发现`pc_update`是和`res_logic`联动的，用于确定返回的res是通过什么样的计算方式得到的。

`pc_update`: 用于标识`pc`是如何更新的，或者可能按照指令直接做加法，也可能是通过跳转的方式更新。

```python
# Compute the new value of pc.
switch pc_update:
    case 0:  # The common case:
        next_pc = pc + instruction_size
    case 1:  # Absolute jump:
        next_pc = res
    case 2:  # Relative jump:
        next_pc = pc + res
    case 4:  # Conditional relative jump (jnz: jump non-zero):
	    # Conditional jumps. 
	    # jmp rel <offset> if <op> != 0
        next_pc = 
	        if dst == 0:
	            pc + instruction_size
	        else:
	            pc + op1
    default:
        Undefined Behavior
```

返回的res决定pc的跳转位置。

`ap_update`: 标识`ap`和`fp`是如何更新的。

```python
# Compute new value of ap and fp based on the opcode.
if opcode == 1:
    # "Call" instruction.
    assert op0 == pc + instruction_size
    assert dst == fp

    # Update fp.
    next_fp = ap + 2

    # Update ap.
    switch ap_update:
        case 0: next_ap = ap + 2
        default: Undefined Behavior
elif opcode is one of 0, 2, 4:
    # Update ap.
    switch ap_update:
        case 0: next_ap = ap
        case 1: next_ap = ap + res
        case 2: next_ap = ap + 1
        default: Undefined Behavior

    switch opcode:
        case 0:
            next_fp = fp
        case 2:
            # "ret" instruction.
            next_fp = dst
        case 4:
            # "assert equal" instruction.
            assert res == dst
            next_fp = fp
else:
    Undefined Behavior
```

`opcode`： 用于表示到底是在执行哪一个opcode指令。

#### fp

通过`fp`的设计来模拟函数调用栈，之前call部分关于`fp`已经有过一点介绍了。

1. `[fp-3], [fp-4]...`存放该函数的输入参数。
2. `[fp-2]`调用函数上一级函数的`fp`位置，用于该函数执行完恢复上一级函数`fp`位置。
3. `[fp-1]`return pc位置，该调用函数执行完上一级函数从哪里继续执行。
4. `[fp], [fp+1]...`该调用函数的local variable。

当函数执行完，将该函数的返回值存放在`[ap-1], [ap-2],...`

### AIR

- **Virtual Column**: A conceptual grouping of trace cells that share the same role in a trace table. These cells are treated as if they are in the same column, even though they might be spread across multiple rows.
    
- **Virtual Subcolumn**: A periodic subset of a virtual column. It allows you to apply more specific constraints to different parts of the data within a virtual column, enabling finer control and organization of the trace data.

#### 9.4 Instruction flags

我们可以对论文中的flag groups也就是instruction structure table最下面一列表示成

$$
\tilde f_i = \sum_{j=i}^{14} 2^{j-i} \cdot f_j
$$
当`i=0`时，$\tilde f_0$ 即表示最下面一列的二进制表示累加和。 $\tilde f_i$ 表示从第i个元素往后算，计算的累加和。自己在纸上可以写一下尝试，很容易验证如下的等式关系

$$
\tilde f_i - 2 \tilde f_{i+1} = f_i
$$

对于 $f_i$ 而言，他的值或者时0，或者是1，所以会有约束

$$
(\tilde f_i - 2 \tilde f_{i+1})(\tilde f_i - 2 \tilde f_{i+1} - 1) = 0
$$

#### 9.5 Updating pc

`pc_update`由3个bit组成，每个bit要么是0要么是1，所以一共有4种状态（3个全部都是0是第四种状态，即`regular_update`），现在认为每一种状态的值或者是0或者是1，4种状态互斥，不可能同时成立，那么这4种状态的互斥关系可以用下式约束。 

```
regular_update = 1 - pc_jump_abs - pc_jump_rel - pc_jnz
```

之前的instruction structure部分已经讲述了`pc_update`值变化规则。

$$
\begin{aligned}
&(1 - f_{\text{PC\_JNZ}}) \cdot \text{next\_pc} \\
&\quad - \left(\text{regular\_update} \cdot (\text{pc} + \text{instruction\_size}) + \right. \\
&\quad \quad f_{\text{PC\_JUMP\_ABS}} \cdot \text{res} + \\
&\quad \quad f_{\text{PC\_JUMP\_REL}} \cdot (\text{pc} + \text{res}) ) = 0
\end{aligned}
$$

该约束就是实现这一点，`regular_update`, `pc_jump_abs`, `pc_jump_rel`为1，那么`next_pc`值应该是多少。不过该约束对`pc_jnz`没有起到约束效果，还需要对`pc_jnz`再增加额外约束。

`pc_jnz`成立时又分成`dst=0`和 $dst \neq 0$  两种情况，对于 $dst \neq 0$ 可以有约束

```
pc_jnz * dst * (next_pc - (pc + op1)) = 0
```

对于`dst=0`可以构造倒数的形式来实现，设置 $v = \text{dst}^{-1}$ 有约束

```
pc_jnz * (dst*v - 1) * (next_pc - (pc + instruction_size)) = 0
```

根据9.3的内容可知，一般约束会表示成2次方的形式，这样做的目的是为了让约束式不至于太过复杂，计算AIR性能比较容易。不过9.5这里我们最终使用的是3次方的形式，不过我们还是要先做一下2次方转换。

```
t_0 = pc_jnz * dst
t_1 = t_0 * v
```

如果将`t_0`，`t_1`看成是1次方（实际上是2次方）那么上述的约束式可以看成是2次方形式。

#### 9.7 and 9.8

该部分内容除了参考论文，还有blog[解读](https://www.cryptologie.net/article/603/cairos-public-memory/)

constraint其实不需要单独设计，如果写的trace是按照正常规则生成的，自然就满足constraint。根据论文的描述，对于原始的memory $L_1 = {(a_i, v_i)}^{n-1}_{i=0}$ 还需要一个额外的辅助memory $L_2 = {(a_i', v_i')}^{n-1}_{i=0}$ 来帮助去证明memory是write-once的。L_2和L_1内容完全一致，所不同的是针对address做了一个排列。在实际代码中也有

```rust
trace_cols.push(extra_addrs);
trace_cols.push(extra_vals);
```

来表示添加这个辅助的 $L_2$ 。约束表示论文中说的也比较明确了。

#### 9.9 Permutation range-check

range check的主要check就是之前的instruction的offset是否是在 $2^{16}$ 范围之内（`dst_off, op0_off, op1_off`）。具体实现思路是对这些数据提取出来，然后排序，可以得到排序后的最小值 $rc_{min}$ 和最大值 $rc_{max}$ ，然后只需要比较这两个值是否在 $2^{16}$ 范围之内就行。（需要一个新的列，命名为`rc_hole`）

memory总体是连续的，但是分成每一个小部分可能有的内容是unused 情况导致那个trace会发生不连续的情况，还有就是build-in加进来的时候可能会导致memory不连续的情况发生，为了保证permutation constraint能够正常运行，就需要对这些空的cell填入dummy值。


## 代码

#### 总体分析

内容参考自go的代码，该项目提供十分丰富的解释，[链接](https://github.com/lambdaclass/cairo-vm_in_go/tree/main)

> What this means is that the point of Cairo is not just to execute some code and get a result, but to _prove_ to someone else that said execution was done correctly, without them having to re-execute the entire thing.

就是说我相信代码执行完之后结果就是它，而不用重新再执行一遍。只需要提交给我一个证明，我验证证明，那么我们就可以对代码的执行过程以及执行结果达成共识。

> Because of this, memory in Cairo is divided into `segments`. This is just a way of organizing memory more conveniently for this write-once model.

Cairo是read的memory结构，写了一次就不能再写了。比如有一个数组，写了一些数据之后可能执行其他的内容，过段时间回来要再对这个数组写入数据，这样该数组的数据是零散的分布在内存里面的，所以我们要实际算trace的时候需要先对memory做一下segments，memory数据按照类别重新排序，这种排序的方式称之为relocation。

> The cairo memory is made up of contiguous segments of variable length identified by their index. The first segment (index 0) is the program segment, which stores the instructions of a cairo program. The following segment (index 1) is the execution segment, which holds the values that are created along the execution of the vm, for example, when we call a function, a pointer to the next instruction after the call instruction will be stored in the execution segment which will then be used to find the next instruction after the function returns. The following group of segments are the builtin segments, one for each builtin used by the program, and which hold values used by the builtin runners. The last group of segments are the user segments, which represent data structures created by the user, for example, when creating an array on a cairo program, that array will be represented in memory as its own segment.

> An address (or pointer) in cairo is represented as a `relocatable` value, which is made up of a `segment_index` and an `offset`, the `segment_index` tells us which segment the value is stored in and the `offset` tells us how many values exist between the start of the segment and the value.

接下来结合go的项目代码讲一下流程。

先抛开built in和hint，那个后面再看，分析除此之外的内容。

整体的代码流程是我写了一段Cairo代码，随后将其转换成json（怎么转换的不考虑），然后将json先翻译成instruction的形式，也就是得到program（instruction就是那个instruction表，填好），此时的memory数据是从instruction的地方来一些（memory是segment的形式，后续会分析）。然后程序执行，register的数值（pc， ap，fp）会有变化，记录下来，然后memory也会有变化，记录下来。程序执行完，再之后将segment的memory改造成contiguous的形式，至此搞定，后面的内容是rust的地方。go分析在于从json到memory改造成contiguous的形式这块。

程序执行的入口在`CairoRun`

```go
func CairoRun(programPath string) (*runners.CairoRunner, error) {
    compiledProgram := parser.Parse(programPath)
    programJson := vm.DeserializeProgramJson(compiledProgram)

    cairoRunner, err := runners.NewCairoRunner(programJson)
    if err != nil {
        return nil, err
    }
    end, err := cairoRunner.Initialize()
    if err != nil {
        return nil, err
    }
    err = cairoRunner.RunUntilPC(end)
    if err != nil {
        return nil, err
    }
    err = cairoRunner.Vm.Relocate()
    return cairoRunner, err
}
```

整个程序的执行流程也来自这个函数，和我之前说的可以对起来。一步一步分析。

**DeserializeProgramJson**

该程序实际上就是把输入的json文件变换成program格式的文件，方便后续处理。来看一下program的定义

```go
type Program struct {
	Data             []memory.MaybeRelocatable
	Builtins         []string
	Identifiers      map[string]Identifier
	Hints            map[uint][]parser.HintParams
	ReferenceManager parser.ReferenceManager
	Start            uint
	End              uint
}

type MaybeRelocatable struct {
    inner any
}

type Relocatable struct {
    SegmentIndex int
    Offset       uint
}
```

关于`maybeRelocatable`如何理解，它可以看成是`Relocatable`的一个模糊版本，可能是地址也可能是值，在go语言里面不好表示，所以用这样的形式来表示。

> As the cairo memory can hold both felts and relocatables, we need a data type that can represent both in order to represent a basic memory unit. We would normally use enums or unions to represent this type, but as go lacks both, we will instead hold a non-typed inner value and rely on the api to make sure we can only create MaybeRelocatable values with either Felt or Relocatable as inner type.

如果内存用映射的形式来表示，那么可以看成是这样

```
    0:0 -> 1
    0:1 -> 4
    0:2 -> 7
    1:0 -> 8
    1:1 -> 0:2
    1:4 -> 0:1
    2:0 -> 1
```

可以看出来，存在内存中的数据可能是数值也可能是指向其他内存位置的地址。

**`NewCairoRunner()`**

```go
func NewCairoRunner(program vm.Program, layoutName string, proofMode bool) (*CairoRunner, error) {
...
	main_offset := uint(0)
	if ok {
		main_offset = uint(mainIdentifier.PC)
	}
	
	runner := CairoRunner{
		Program:    program,
		Vm:         *vm.NewVirtualMachine(),
		mainOffset: main_offset,
	}
	return &runner, nil
}
```

略去非核心的内容，new runner的作用就是构造这样一个数据结构。

看一下`*vm.NewVirtualMachine()`，该函数的作用是对虚拟机做初始化，先忽略built in。

```go
func NewVirtualMachine() *VirtualMachine {
	segments := memory.NewMemorySegmentManager()
	builtin_runners := make([]builtins.BuiltinRunner, 0, 9) // There will be at most 9 builtins
	trace := make([]TraceEntry, 0)
	relocatedTrace := make([]RelocatedTraceEntry, 0)
	return &VirtualMachine{Segments: segments, BuiltinRunners: builtin_runners, Trace: trace, RelocatedTrace: relocatedTrace}
}
```

首先看一下`NewMemorySegmentManager()`，需要注意的是segment是在程序还在json的时候就已经做了。接下来程序执行完到relocated之前会一直保持segment状态。

> Segments are identified by an `index`, an integer value that uniquely identifies them.

segment的划分数据样貌之前例子中看到了。

```go
func NewMemorySegmentManager() MemorySegmentManager {
	memory := NewMemory()
	return MemorySegmentManager{make(map[uint]uint), make(map[uint]uint), *memory, make(map[uint][]uint)}
}

func NewMemory() *Memory {
	return &Memory{
		Data:              make(map[Relocatable]MaybeRelocatable),
		validatedAdresses: NewAddressSet(),
		validationRules:   make(map[uint]ValidationRule),
		AccessedAddresses: make(map[Relocatable]bool),
	}
}

type Memory struct {
	Data              map[Relocatable]MaybeRelocatable
	numSegments       uint
	validationRules   map[uint]ValidationRule
	validatedAdresses AddressSet
	// This is a map of addresses that were accessed during execution
	// The map is of the form `segmentIndex` -> `offset`. This is to
	// make the counting of memory holes easier
	AccessedAddresses map[Relocatable]bool
}

// MemorySegmentManager manages the list of memory segments.
// Also holds metadata useful for the relocation process of
// the memory at the end of the VM run.
type MemorySegmentManager struct {
	SegmentUsedSizes map[uint]uint
	SegmentSizes     map[uint]uint
	Memory           Memory
	// In the original vm implementation, public memory is a list of tuples (uint, uint).
	// The thing is, that second uint is ALWAYS zero. Every single single time someone instantiates
	// some public memory, that second value is zero. I just removed it.
	PublicMemoryOffsets map[uint][]uint
}
```

首先看一下Memory的数据结构`type Memory`，可以看到memory存储的数据结构是`Relocatable` -> `MaybeRelocatable`就是之前举的例子比如说是`0:0 -> 1`。

接下来是Memory Segment Manager数据结构`type MemorySegmentManager`，它的作用是:

> In our `Memory` implementation, it looks like we need to have segments allocated before performing any valid memory operation, but we can't do so from the `Memory` api. To do so, we need to use the `MemorySegmentManager`. The `MemorySegmentManager` is in charge of creating new segments and calculating their size during the relocation process

现在`*vm.NewVirtualMachine()`执行完，各个数据结构做了初始化，但是目前里面内容是空的，还没有任何值，要在程序执行期间填入具体内容。

**`cairoRunner.Initialize()`**

看一下initialize的代码

```go
// Performs the initialization step, returns the end pointer (pc upon which execution should stop)
func (r *CairoRunner) Initialize() (memory.Relocatable, error) {
	err := r.InitializeBuiltins()
	if err != nil {
		return memory.Relocatable{}, errors.New(err.Error())
	}
	r.InitializeSegments()
	end, err := r.initializeMainEntrypoint()
	if err == nil {
		err = r.initializeVM()
	}
	return end, err
}
```

还是builtin等会再说，先是`r.InitializeSegments()`，

```go
// Creates program, execution and builtin segments
func (r *CairoRunner) InitializeSegments() {
	// Program Segment: {0:0}
	r.ProgramBase = r.Vm.Segments.AddSegment()
	// Execution Segment: {1:0}
	r.executionBase = r.Vm.Segments.AddSegment()
	// Builtin Segments
	for i := range r.Vm.BuiltinRunners {
		r.Vm.BuiltinRunners[i].InitializeSegments(&r.Vm.Segments)
	}
}

// Adds a memory segment and returns the first address of the new segment
func (m *MemorySegmentManager) AddSegment() Relocatable {
	ptr := Relocatable{int(m.Memory.numSegments), 0}
	m.Memory.numSegments += 1
	// ptr means pointer
	return ptr
}
```

该函数的作用就是获得初始化的segment的pointer的地址。比如programBase的segment地址就是`(0:0)`，executionBase的地址就是`(1:0)`。

接下来是`r.initializeMainEntrypoint()`

```go
// Initializes memory, initial register values & returns the end pointer (final pc) to run from the main entrypoint
func (r *CairoRunner) initializeMainEntrypoint() (memory.Relocatable, error) {
	// When running from main entrypoint, only up to 11 values will be written (9 builtin bases + end + return_fp)
	stack := make([]memory.MaybeRelocatable, 0, 11)
	// Append builtins initial stack to stack
	for i := range r.Vm.BuiltinRunners {
		for _, val := range r.Vm.BuiltinRunners[i].InitialStack() {
			stack = append(stack, val)
		}
	}

	// return_fp segement: {2:0}
	return_fp := *memory.NewMaybeRelocatableRelocatable(r.Vm.Segments.AddSegment())
	return r.initializeFunctionEntrypoint(r.mainOffset, &stack, return_fp)
}

// Initializes memory, initial register values & returns the end pointer (final pc) to run from a given pc offset
// (entrypoint)
func (r *CairoRunner) initializeFunctionEntrypoint(entrypoint uint, stack *[]memory.MaybeRelocatable, return_fp memory.MaybeRelocatable) (memory.Relocatable, error) {
	// end segment: {3:0}
	end := r.Vm.Segments.AddSegment()
	*stack = append(*stack, return_fp, *memory.NewMaybeRelocatableRelocatable(end))
	// {1:0}
	r.initialFp = r.executionBase
	r.initialFp.Offset += uint(len(*stack))
	// {1: len(stack)}
	r.initialAp = r.initialFp
	r.finalPc = &end
	return end, r.initializeState(entrypoint, stack)
}
```

`r.initializeMainEntrypoint()`函数的作用：

> This method will initialize the memory and initial register values to begin execution from the main entry point, and return the final pc

该函数首先对built in做一些stack生成，先不去管，然后是main函数return fp的位置，也生成一个新的segment。接下来进入到`r.initializeFunctionEntrypoint()`函数，该函数的作用是

该函数的作用是对memory和register做初始化，向vm写入memory数据（program instruction，execution stack初始化的值），以及获得main函数（entry point）的pc位置。

看一下`r.initializeFunctionEntrypoint()`函数最后一行的`r.initializeState()`函数

```go
// Initializes the program segment & initial pc
func (r *CairoRunner) initializeState(entrypoint uint, stack *[]memory.MaybeRelocatable) error {
	// {0:0}
	r.initialPc = r.ProgramBase
	// {0:main_offset}
	r.initialPc.Offset += entrypoint
	// Load program data
	_, err := r.Vm.Segments.LoadData(r.ProgramBase, &r.Program.Data)
	if err == nil {
		_, err = r.Vm.Segments.LoadData(r.executionBase, stack)
	}
	// Mark data segment as accessed
	base := r.ProgramBase
	var i uint
	for i = 0; i < uint(len(r.Program.Data)); i++ {
		r.Vm.Segments.Memory.MarkAsAccessed(memory.NewRelocatable(base.SegmentIndex, base.Offset+i))
	}
	return err
}

// Writes data into the memory from address ptr and returns the first address after the data.
// If any insertion fails, returns (0,0) and the memory insertion error
func (m *MemorySegmentManager) LoadData(ptr Relocatable, data *[]MaybeRelocatable) (Relocatable, error) {
	for _, val := range *data {
		err := m.Memory.Insert(ptr, &val)
		if err != nil {
			return Relocatable{0, 0}, err
		}
		ptr.Offset += 1
	}
	return ptr, nil
}

func (m *Memory) Insert(addr Relocatable, val *MaybeRelocatable) error {
    // Check that insertions are preformed within the memory bounds
    if addr.segmentIndex >= int(m.num_segments) {
        return errors.New("Error: Inserting into a non allocated segment")
    }

    // Check for possible overwrites
    prev_elem, ok := m.data[addr]
    if ok && prev_elem != *val {
        return errors.New("Memory is write-once, cannot overwrite memory value")
    }

    m.data[addr] = *val

    return nil
}
```

可以看到此时initial pc的位置已经确定了是`{0: main_offset}`，然后接下来Load program data是把program的指令写入vm对应program（也就是`{0:0}`之后）的segment。从`LoadData()`函数可以看到遍历data然后写入。原先是map现在转换之后就是新的形式。比如说

```
0:0 -> 1
0:1 -> 2

// new
[1, 2]
```

这样的一个转换。execution data的写入对应的segment也是同样的道理，所不同的是program segment写入的是instruction，execution的segment写入的是stack的数据。

`r.initializeMainEntrypoint()`结束后是`r.initializeVM()`

```go
// Initializes the vm's run_context, adds builtin validation rules & validates memory
func (r *CairoRunner) initializeVM() error {
	// {1: len(stack)}
	r.Vm.RunContext.Ap = r.initialAp
	// {1:0}
	r.Vm.RunContext.Fp = r.initialFp
	// {0:0}
	r.Vm.RunContext.Pc = r.initialPc
	// Add validation rules
	for i := range r.Vm.BuiltinRunners {
		r.Vm.BuiltinRunners[i].AddValidationRule(&r.Vm.Segments.Memory)
	}
	// Apply validation rules to memory
	return r.Vm.Segments.Memory.ValidateExistingMemory()
}
```

该函数是对运行时vm的register赋初始值。

**`cairoRunner.RunUntilPC()`**

初始化之后就是执行整个虚拟机。

```go
func (r *CairoRunner) RunUntilPC(end memory.Relocatable, hintProcessor vm.HintProcessor) error {
	hintDataMap, err := r.BuildHintDataMap(hintProcessor)
	if err != nil {
		return err
	}
	constants := r.Program.ExtractConstants()
	for r.Vm.RunContext.Pc != end &&
		(r.Vm.RunResources == nil || !r.Vm.RunResources.Consumed()) {
		err := r.Vm.Step(hintProcessor, &hintDataMap, &constants, &r.execScopes)
		if err != nil {
			return err
		}
		if r.Vm.RunResources != nil {
			r.Vm.RunResources.ConsumeStep()
		}
	}
	if r.Vm.RunContext.Pc != end {
		return errors.New("Could not reach the end of the program. RunResources has no remaining steps.")
	}
	return nil
}
```

不管别的，只看核心代码`Step()`函数，

```go
func (v *VirtualMachine) Step(hintProcessor HintProcessor, hintDataMap *map[uint][]any, constants *map[string]lambdaworks.Felt, execScopes *types.ExecutionScopes) error {
	// Run Hint
	hintDatas, ok := (*hintDataMap)[v.RunContext.Pc.Offset]
	if ok {
		for i := 0; i < len(hintDatas); i++ {
			err := hintProcessor.ExecuteHint(v, &hintDatas[i], constants, execScopes)
			if err != nil {
				return err
			}
		}
	}

	// Run Instruction
	encoded_instruction, err := v.Segments.Memory.Get(v.RunContext.Pc)
	if err != nil {
		return fmt.Errorf("Failed to fetch instruction at %+v", v.RunContext.Pc)
	}

	encoded_instruction_felt, ok := encoded_instruction.GetFelt()
	if !ok {
		return errors.New("Wrong instruction encoding")
	}

	encoded_instruction_uint, err := encoded_instruction_felt.ToU64()
	if err != nil {
		return err
	}

	instruction, err := DecodeInstruction(encoded_instruction_uint)
	if err != nil {
		return err
	}

	return v.RunInstruction(&instruction)
}
```

该函数首先通过`v.Segments.Memory.Get()`方式获得initial pc所指向的指令数据（还是map的方式`{0:0} -> val`，之前的loadData写入应该只是为了将来做relocated）。经过一系列类型转换之后val变成了`encoded_instruction_uint`，接着进入到`DecodeInstruction(encoded_instruction_uint)`，该函数实际上就是对val这个instruction解构，变成之前说的那个instruction的图表。该函数的具体内容不需要关注，只需要看一下返回值

```go
func DecodeInstruction(encodedInstruction uint64) (Instruction, error) {
	...
	return Instruction{
	// dst_offset
	Off0:     offset0, // base+off0 -> val
	Off1:     offset1,
	Off2:     offset2,
	DstReg:   dstRegister,
	Op0Reg:   op0Register,
	Op1Addr:  op1Src,
	ResLogic: res,
	PcUpdate: pcUpdate,
	ApUpdate: apUpdate,
	FpUpdate: fpUpdate,
	Opcode:   opcode,
	}, nil
}
```

其内容和图标没有什么区别，唯一不同的是图表只有`ap_update`因为有的时候是这个其实是fp而不是ap，所以可以表示分成上述代码的`ApUpdate`和`FpUpdate`。还有一点需要注意的是`off0`之类的寄存器存储的是地址的偏移值，而不是实际数值。实际的val要通过base+offset的方式在map中查出来。

有了instruction之后接下来执行`v.RunInstruction(&instruction)`，具体执行这个指令。简化版代码如下

```go
func (v *VirtualMachine) RunInstruction(instruction *Instruction) error {
	operands, err := v.ComputeOperands(*instruction)
	if err != nil {
		return err
	}

	err = v.OpcodeAssertions(*instruction, operands)
	if err != nil {
		return err
	}

	v.Trace = append(v.Trace, TraceEntry{Pc: v.RunContext.Pc, Ap: v.RunContext.Ap, Fp: v.RunContext.Fp})

	err = v.UpdateRegisters(instruction, &operands)
	if err != nil {
		return err
	}

	v.CurrentStep++
	return nil
}
```

首先是`v.ComputeOperands(*instruction)`，该函数的功能是

> This function is in charge of calculating the addresses of the operands and fetching them from memory. If the function could not fetch the operands then they are deduced from the other operands, taking in consideration what kind of opcode is being executed.

```go
func (vm *VirtualMachine) ComputeOperands(instruction Instruction) (Operands, error) {
    var res *memory.MaybeRelocatable

    dst_addr, err := vm.RunContext.ComputeDstAddr(instruction)
    if err != nil {
        return Operands{}, errors.New("FailedToComputeDstAddr")
    }
    dst, _ := vm.Segments.Memory.Get(dst_addr)

    op0_addr, err := vm.RunContext.ComputeOp0Addr(instruction)
    if err != nil {
        return Operands{}, fmt.Errorf("FailedToComputeOp0Addr: %s", err)
    }
    op0, _ := vm.Segments.Memory.Get(op0_addr)

    op1_addr, err := vm.RunContext.ComputeOp1Addr(instruction, op0)
    if err != nil {
        return Operands{}, fmt.Errorf("FailedToComputeOp1Addr: %s", err)
    }
    op1, _ := vm.Segments.Memory.Get(op1_addr)

    if op0 == nil {
        deducedOp0, deducedRes, err := vm.DeduceOp0(&instruction, dst, op1)
        if err != nil {
            return Operands{}, err
        }
        op0 = deducedOp0
        if op0 != nil {
            vm.Segments.Memory.Insert(op0_addr, op0)
        }
        res = deducedRes
    }

    if op1 == nil {
        deducedOp1, deducedRes, err := vm.DeduceOp1(instruction, dst, op0)
        if err != nil {
            return Operands{}, err
        }
        op1 = deducedOp1
        if op1 != nil {
            vm.Segments.Memory.Insert(op1_addr, op1)
        }
        if res == nil {
            res = deducedRes
        }
    }

    if res == nil {
        res, err = vm.ComputeRes(instruction, *op0, *op1)

        if err != nil {
            return Operands{}, err
        }
    }

    if dst == nil {
        deducedDst := vm.DeduceDst(instruction, res)
        dst = deducedDst
        if dst != nil {
            vm.Segments.Memory.Insert(dst_addr, dst)
        }
    }

    operands := Operands{
        Dst: *dst,
        Op0: *op0,
        Op1: *op1,
        Res: res,
    }
	operandsAddresses := OperandsAddresses{
		DstAddr: dstAddr,
		Op0Addr: op0Addr,
		Op1Addr: op1Addr,
	}
	return operands, operandsAddresses, nil
}

func (vm *VirtualMachine) DeduceDst(instruction Instruction, res *memory.MaybeRelocatable) *memory.MaybeRelocatable {
    switch instruction.Opcode {
    case AssertEq:
        return res
    case Call:
        return memory.NewMaybeRelocatableRelocatable(vm.RunContext.Fp)

    }
    return nil
}
```

从代码中可以得知，首先通过之前的offset和base结合起来，然后得到dst addr，operand 0还有operand 1的addr，然后结合addr获取operand 0以及operand 1的val。res的值计算方式是通过`vm.ComputeRes(instruction, *op0, *op1)`在该函数中，可以通过instruction知道是执行对op0和op1的加法还是乘法或者是别的计算，得到res的结果。有了res的结果，就可以根据指令在`vm.DeduceDst(instruction, res)`得到dst的结果到底是多少，可能是res也可能是别的值。有了dst的值，也有dst的地址，就把新得到的dst值放到dst的地址里面去。随后返回得到的4个operand操作数以及3个操作数的地址。

接下来是`v.OpcodeAssertions(*instruction, operands)`该函数的作用是确保程序得到了正确的执行。

> Once we have the instruction's operands to work with, we have to ensure the correctness of them. If this method returns a nil error, it means operands were computed correctly and we are good to go!

然后更新Trace，实际上就是register的table，`v.Trace`里面添加之前用于instruction计算的pc，ap，fp值。因为后面这些值会变，所以提前添加到trace表里做保存。（注意pc的值始终是在program segment里面变化，而ap和fp，是在evaluation segment甚至之后的semgment变化。）

然后是`v.UpdateRegisters(instruction, &operands)`用于更新register

```go
// Updates the values of the RunContext's registers according to the executed instruction
func (vm *VirtualMachine) UpdateRegisters(instruction *Instruction, operands *Operands) error {
	if err := vm.UpdateFp(instruction, operands); err != nil {
		return err
	}
	if err := vm.UpdateAp(instruction, operands); err != nil {
		return err
	}
	return vm.UpdatePc(instruction, operands)
}
```

vm对应的fp，pc，ap值受到当前的instruction和计算完的operand，下一步的fp，pc，ap会发生一些变化，通过这个函数计算出下一步的fp，pc，ap值是多少。

随后currentStep+1，

**`cairoRunner.Vm.Relocate()`**

看一下该函数的解释

> This method will relocate the memory and trace generated by the program execution. Relocating means that our VM transforms a two-dimensional memory (aka a memory divided by segments) to a continuous, one-dimensional memory. In this section, we will refer to the two-dimensional memory as the original memory and the one-dimensional memory as the relocated memory.

```go
func (v *VirtualMachine) Relocate() error {
	v.Segments.ComputeEffectiveSizes()
	if len(v.Trace) == 0 {
		return nil
	}

	relocationTable, err := v.Segments.RelocateSegments()
	// This should be unreachable
	if err != nil {
		return errors.New("ComputeEffectiveSizes called but RelocateSegments still returned error")
	}

	relocatedMemory, err := v.Segments.RelocateMemory(&relocationTable)
	if err != nil {
		return err
	}

	v.RelocateTrace(&relocationTable)
	v.RelocatedMemory = relocatedMemory
	return nil
}
```

首先进入到`v.Segments.ComputeEffectiveSizes()`，该函数的作用是

> Calculates the size of each memory segment.

该函数返回的是一个map： segmentIndex -> segmentSize，这样就可以知道每一个的segment有多少个元素。

随后是`v.Segments.RelocateSegments()`，该函数的作用是

> Returns a vector containing the first relocated address of each memory segment. 
> 
> Because we want the relocated memory to be continuous, the segments should be placed one after the other. This means that the last address of the segment `i` is followed by the first address of the segment `i + 1`. To know where to relocate the segments, we need to know the first address of each segment as if they were already relocated.

这个函数返回一个数组，数组里的元素是在重排列情况下的起始地址，比如说是`[1, segment_0_initAddr,  segment_1_initAddr, ...]`，这些地址的计算方式就是通过从最初的地址开始然后加segment size，然后就可以知道下一个segment的起始地址是多少了。

再之后是`v.Segments.RelocateMemory(&relocationTable)`，该函数的作用是返回一个`uint -> finite field`的映射组。首先是uint的这个key部分，它其实就是base+offset的组合，比如说`{0:1}`会被转换成1，`{1:2}`会被转换成3。这样来保证转换后索引的连续性。然后是val的部分，如果是值那么就直接是值，如果也是`{a:b}`这种形式的话，就继续往后去找，直到找到值为止，保证组成`uint -> finite field`。

最后是`v.RelocateTrace(&relocationTable)`，该函数的作用是

> Relocates the VM's trace, turning relocatable registers to numbered ones. Because those fields are address, the trace relocation process involves relocating addresses. That's why, this method also calls `RelocateAddress`

```go
// Relocates the VM's trace, turning relocatable registers to numbered ones
func (v *VirtualMachine) RelocateTrace(relocationTable *[]uint) error {
	if len(*relocationTable) < 2 {
		return errors.New("No relocation found for execution segment")
	}

	for _, entry := range v.Trace {
		v.RelocatedTrace = append(v.RelocatedTrace, RelocatedTraceEntry{
			Pc: lambdaworks.FeltFromUint64(uint64(entry.Pc.RelocateAddress(relocationTable))),
			Ap: lambdaworks.FeltFromUint64(uint64(entry.Ap.RelocateAddress(relocationTable))),
			Fp: lambdaworks.FeltFromUint64(uint64(entry.Fp.RelocateAddress(relocationTable))),
		})
	}

	return nil
}
```

我们之前在`RunInstruction`过程中添加了pc，ap，fp的值，但是目前该值都是`{a:b}`的形式，需要将其转换成a+b的这种形式然后保存，所以这里做一个转换工作。

#### built in部分

接下来进入到built in部分，上述代码的部分内容再走一遍。

从程序设计的角度来讲，可以认为vm内部的built in定义了一系列的接口，在外部任何实现该类型的程序都可以认为是一种built in，这样就把built in能力外放，给vm程序运行提供了灵活性。

```go
type BuiltinRunner interface {
    // Returns the first address of the builtin's memory segment
    Base() memory.Relocatable
    // Returns the name of the builtin
    Name() string
    // Creates a memory segment for the builtin and initializes its base
    InitializeSegments(*memory.MemorySegmentManager)
    // Returns the builtin's initial stack
    InitialStack() []memory.MaybeRelocatable
    // Attempts to deduce the value of a memory cell given by its address. Can return either a nil pointer and an error, if an error arises during the deduction,
    // a valid pointer and nil if the deduction was succesful, or a nil pointer and nil if there is no deduction for the memory cell
    DeduceMemoryCell(memory.Relocatable, *memory.Memory) (*memory.MaybeRelocatable, error)
    // Adds a validation rule to the memory
    // Validation rules are applied when a value is inserted into the builtin's segment
    AddValidationRule(*memory.Memory)
}
```

**`NewCairoRunner()`**

在该函数中有`func CheckBuiltinsSubsequence(programBuiltins []string) error`用来检查json的文件里面的built in是否是预定义的built in，如果不是就报错，目前预定义的built in支持以下几种

```go
orderedBuiltinNames := []string{
	"output",
	"pedersen",
	"range_check",
	"ecdsa",
	"bitwise",
	"ec_op",
	"keccak",
	"poseidon",
}
```

**`cairoRunner.Initialize()`**

```go
func (r *CairoRunner) Initialize() (memory.Relocatable, error) {
	err := r.InitializeBuiltins()
	if err != nil {
		return memory.Relocatable{}, errors.New(err.Error())
	}
	r.InitializeSegments()
	end, err := r.initializeMainEntrypoint()
	if err == nil {
		err = r.initializeVM()
	}
	return end, err
}
```

在该函数中有对built in的initialize，`err := r.InitializeBuiltins()`去做一些初始化的工作。

看一下`r.InitializeSegments()`代码，

```go
func (r *CairoRunner) InitializeSegments() {
	...
	// Builtin Segments
	for i := range r.Vm.BuiltinRunners {
		r.Vm.BuiltinRunners[i].InitializeSegments(&r.Vm.Segments)
	}
}

// range check for example
func (r *RangeCheckBuiltinRunner) InitializeSegments(segments *memory.MemorySegmentManager) {
	r.base = segments.AddSegment()
}
```

segment建立的时候对于built in而言，要各个对应的built in自己实现对应的`InitializeSegments()`方法，这样看来每一个built in都会建立自己的segment。

随后是`r.initializeVM()`，该函数的作用如下

```go
func (r *CairoRunner) initializeVM() error {
	...
	// Add validation rules
	for i := range r.Vm.BuiltinRunners {
	// AddValidationRule is interface method
	r.Vm.BuiltinRunners[i].AddValidationRule(&r.Vm.Segments.Memory)
	}
	// Apply validation rules to memory
	return r.Vm.Segments.Memory.ValidateExistingMemory()
}

// Applies validation_rules to every memory address, if applicatble
// Skips validation if the address is temporary or if it has been previously validated
func (m *Memory) ValidateExistingMemory() error {
	for addr := range m.Data {
		err := m.validateAddress(addr)
		if err != nil {
			return err
		}
	}
	return nil
}
```

> Here we will add our builtin's validation rules to the `Memory` and use them to validate the memory cells we loaded before.

需要注意的是`AddValidationRule`是一个接口，具体实现在built in里面。下面的代码是以range check为例。

```go
func (r *RangeCheckBuiltinRunner) AddValidationRule(mem *memory.Memory) {
	mem.AddValidationRule(uint(r.base.SegmentIndex), RangeCheckValidationRule)
}

// Adds a validation rule for a given segment
func (m *Memory) AddValidationRule(SegmentIndex uint, rule ValidationRule) {
	m.validationRules[SegmentIndex] = rule
}

// A function that validates a memory address and returns a list of validated addresses
type ValidationRule func(*Memory, Relocatable) ([]Relocatable, error)

func RangeCheckValidationRule(mem *memory.Memory, address memory.Relocatable) ([]memory.Relocatable, error) {
	res_val, err := mem.Get(address)
	if err != nil {
		return nil, err
	}
	felt, is_felt := res_val.GetFelt()
	if !is_felt {
		return nil, NotAFeltError(address, *res_val)
	}
	if felt.Bits() <= RANGE_CHECK_N_PARTS*INNER_RC_BOUND_SHIFT {
		return []memory.Relocatable{address}, nil
	}
	return nil, OutsideBoundsError(felt)
}
```

首先来看一下validation rule是什么。

> Builtins have two ways to operate: via validation rules and via auto-deduction rules. Validation rules are applied to every element that is inserted into a builtin's segment. For example, if I want to verify an ecdsa signature, I can insert it into the ecdsa builtin's segment and let a validation rule take care of verifying the signature.

> This will represent our builtin's validation rules, they take a memory address and a reference to the memory, and return a list of validated addresses, _for most builtins, this list will contain the address it received if the validation was successful but some builtins may return additional addresses.

从上述代码可以看出，validation rule是定义在具体的built in里面的，作用是验证built in主张的数据是否满足对应关系。比如说对于range check而言，要求`addr -> val`中val需要在range范围之内，如果满足的话，就把addr的值返回出去，存在memory所对应的数组里面。

**`cairoRunner.RunUntilPC()`**

在该部分有代码

```go
func (v *VirtualMachine) RunInstruction(instruction *Instruction) error {
	operands, err := v.ComputeOperands(*instruction)
	...

func (vm *VirtualMachine) ComputeOperands(instruction Instruction) (Operands, OperandsAddresses, error) {
	...
	var op0 memory.MaybeRelocatable
	if op0Op != nil {
		op0 = *op0Op
	} else {
		op0, res, err = vm.ComputeOp0Deductions(op0Addr, &instruction, dst, op1Op)
		if err != nil {
			return Operands{}, OperandsAddresses{}, err
		}
	}

	var op1 memory.MaybeRelocatable
	if op1Op != nil {
		op1 = *op1Op
	} else {
		op1, err = vm.ComputeOp1Deductions(op1Addr, &instruction, dst, op0Op, res)
		if err != nil {
			return Operands{}, OperandsAddresses{}, err
		}
	}
	...
}
```

其中`ComputeOperands()`有一些case会涉及built in，看一下`vm.ComputeOp0Deductions()`函数

```go
// Runs deductions for Op0, first runs builtin deductions, if this fails, attempts to deduce it based on dst and op1
// Also returns res if it was also deduced in the process
// Inserts the deduced operand
// Fails if Op0 was not deduced or if an error arose in the process
func (vm *VirtualMachine) ComputeOp0Deductions(op0_addr memory.Relocatable, instruction *Instruction, dst *memory.MaybeRelocatable, op1 *memory.MaybeRelocatable) (deduced_op0 memory.MaybeRelocatable, deduced_res *memory.MaybeRelocatable, err error) {
	op0, err := vm.DeduceMemoryCell(op0_addr)
	if err != nil {
		return *memory.NewMaybeRelocatableFelt(lambdaworks.FeltZero()), nil, err
	}
	if op0 == nil {
		op0, deduced_res, err = vm.DeduceOp0(instruction, dst, op1)
		if err != nil {
			return *memory.NewMaybeRelocatableFelt(lambdaworks.FeltZero()), nil, err
		}
	}
	if op0 != nil {
		vm.Segments.Memory.Insert(op0_addr, op0)
	} else {
		return *memory.NewMaybeRelocatableFelt(lambdaworks.FeltZero()), nil, errors.New("Failed to compute or deduce op0")
	}
	return *op0, deduced_res, nil
}
```

首先来介绍一下built in deduction的概念

> Auto-deduction rules take over during instruction execution, when we can't compute the value of an operand who's address belongs to a builtin segment, we can use that builtin's auto-deduction rule to calculate the value of the operand. For example, If I want to calculate the pedersen hash of two values, I can write the values into the pedersen builtin's segment and then ask for the next memory cell, without builtins, this instruction would have failed, as there is no value stored in that cell, but now we can use auto-deduction rules to calculate the hash and fill in that memory cell.

> Before builtins, the basic flow for computing the value of an operand was to first compute its address, and then if we couldn't find it in memory, we would deduce its value based on the other operands. With the introduction of builtins and their auto-deduction rules, this flow changes a bit. Now we compute the address, use it to fetch the value from memory, if we can't find it in memory we try to use the builtin's auto deduction rules, and if we can't deduce it via builtins we will then deduce it based on the other operands's.

从`RunInstruction()`的代码可以看出，有些情况下，`op0，res，op1`的值不是来自内存或者指令计算而是来自built in的推断，推断这个概念上面讲了，接下来看一下具体是如何实现推断的。

```go
// Applies the corresponding builtin's deduction rules if addr's segment index corresponds to a builtin segment
// Returns nil if there is no deduction for the address
func (vm *VirtualMachine) DeduceMemoryCell(addr memory.Relocatable) (*memory.MaybeRelocatable, error) {
	if addr.SegmentIndex < 0 {
		return nil, nil
	}
	for i := range vm.BuiltinRunners {
		if vm.BuiltinRunners[i].Base().SegmentIndex == addr.SegmentIndex {
			return vm.BuiltinRunners[i].DeduceMemoryCell(addr, &vm.Segments.Memory)
		}
	}
	return nil, nil
}

// Poseidon for example
func (p *PoseidonBuiltinRunner) DeduceMemoryCell(address memory.Relocatable, mem *memory.Memory) (*memory.MaybeRelocatable, error) {
	// Check if its an input cell
	index := address.Offset % POSEIDON_CELLS_PER_INSTANCE
	if index < POSEIDON_INPUT_CELLS_PER_INSTANCE {
		return nil, nil
	}

	value, ok := p.cache[address]
	if ok {
		return memory.NewMaybeRelocatableFelt(value), nil
	}
	// index will always be less or equal to address.Offset so we can ignore the error
	input_start_addr, _ := address.SubUint(index)
	output_start_address := input_start_addr.AddUint(POSEIDON_INPUT_CELLS_PER_INSTANCE)

	// Build the initial poseidon state
	var poseidon_state [POSEIDON_INPUT_CELLS_PER_INSTANCE]lambdaworks.Felt

	for i := uint(0); i < POSEIDON_INPUT_CELLS_PER_INSTANCE; i++ {
		felt, err := mem.GetFelt(input_start_addr.AddUint(i))
		if err != nil {
			return nil, err
		}
		poseidon_state[i] = felt
	}

	// Run the poseidon permutation
	starknet_crypto.PoseidonPermuteComp(&poseidon_state)

	// Insert the new state into the corresponding output cells in the cache
	for i, elem := range poseidon_state {
		p.cache[output_start_address.AddUint(uint(i))] = elem
	}
	return memory.NewMaybeRelocatableFelt(p.cache[address]), nil
}
```

可以看到如果指令的addr segment和built in的对应segment index是一致的，说明op的值要来自built in，所谓的deduce也是built in的一个计算而已，通过built in内部把这个值算出来，然后返回给op。

总的来说，built in有两种，一种是validation rules，就是我提前在built in对应的segment里面填入数据，然后程序执行时built in起作用来验证我这个built in是不是满足built in规则的数据（如range check），还有一种是auto-deduction rules，就是built in里面没有数据，我可能计算某个operand的时候触发built in规则，然后我把数据给某个built in然后built in帮我把结果算出来吐给我，随后将数据赋值给operand（如Poseidon hash）。

#### Hints 部分

Hint和built in非常像，可以看成是简化版的built in，或者是灵活版的built in，用于实现自己想要实现的某些细节功能。看一下具体解释。

> A `Hint` is a piece of code that is not proven, and therefore not seen by the verifier. If `fib` above were a hint, then the prover could convince the verifier that `result` is 144, 0, 1000 or any other number.

整个Hint流程如下：

> Present the result of the calculation to the verifier through a hint, then show said result indeed satisfies the relevant condition that makes it the actual result.
> 
> _Notice that the last assert is absolutely mandatory to make this safe_. If you forget to write it, the square root calculation does not get proven, and anyone could convince the verifier that the result of `sqrt(x)` is any number they like.

也就是分成两步，第一步是执行外部程序向VM中写入数据，第二步是验证该写入的程序正确性。

举个例子比如说`sqrt(x)`的计算，如果放在vm里面的话，需要很多条指令才能算出来，所以我们把计算放在vm的外面，只是把算完的结果放在vm里面，然后验证`sqrt(x)^2=x`是否成立，乘积的计算指令是十分简单的。从而达到简化约束的目的。

但是这样Hint的使用可能会导致一个问题，称之为non-deterministic，上面的sqrt例子十分明显，`sqrt(x)`不管是正数还是负数都可以通过验证，导致vm的实际内存结果可能不唯一。所以如果使用hint不当会给VM带来很大的安全隐患。对此的应对方案是一般不让程序的开发者去接触Hint。

> As explained above, using hints in your code is highly unsafe. Forgetting to add a check after calling them can make your code vulnerable to any sorts of attacks, as your program will not prove what you think it proves.
> 
> Because of this, most hints in Cairo are wrapped around or used by functions in the Cairo common library that do the checks for you, thus making them safe to use. Ideally, Cairo developers should not be using hints on their own; only transparently through Cairo library functions they call.

下面看一下Hint是如何实现的，同built in类似，也是vm里面预留接口，供外部来去调用。Hint接口提供两个方法

```go
type HintProcessor interface {
	// Transforms hint data outputed by the VM into whichever format will be later used by ExecuteHint
	CompileHint(hintParams *parser.HintParams, referenceManager *parser.ReferenceManager) (any, error)
	// Executes the hint which's data is provided by a dynamic structure previously created by CompileHint
	ExecuteHint(vm *VirtualMachine, hintData *any, constants *map[string]lambdaworks.Felt, execScopes *types.ExecutionScopes) error
}
```

**`cairoRunner.RunUntilPC()`**

Hint最早出现是在程序开始execute之后，initialize时没有hint的事情。

```go
func (r *CairoRunner) RunUntilPC(end memory.Relocatable, hintProcessor vm.HintProcessor) error {
	hintDataMap, err := r.BuildHintDataMap(hintProcessor)
	if err != nil {
		return err
	}
	constants := r.Program.ExtractConstants()
	for r.Vm.RunContext.Pc != end &&
		(r.Vm.RunResources == nil || !r.Vm.RunResources.Consumed()) {
		err := r.Vm.Step(hintProcessor, &hintDataMap, &constants, &r.execScopes)
		if err != nil {
			return err
		}
		if r.Vm.RunResources != nil {
			r.Vm.RunResources.ConsumeStep()
		}
	}
	if r.Vm.RunContext.Pc != end {
		return errors.New("Could not reach the end of the program. RunResources has no remaining steps.")
	}
	return nil
}
```

在`r.BuildHintDataMap()`和`r.Vm.Step()`处都会使用Hint。后面的先不分析了。



assert是如何实现的？用lookup吗



#### 入口

以该Cario程序为例去做分析

```rust
func main() {
	let x = 1;
	let y = 2;
	assert x + y = 3;
	return ();
}
```

程序的入口函数在`tests/utils.rs`的`test_prove_cairo_program`函数。

```rust
pub fn test_prove_cairo_program(file_path: &str, layout: CairoLayout) {
    let proof_options = ProofOptions::default_test_options();
    let timer = Instant::now();
    println!("Making proof ...");

    let program_content = std::fs::read(file_path).unwrap();
    let (main_trace, pub_inputs) = generate_prover_args(&program_content, layout).unwrap();
    let proof = generate_cairo_proof(&main_trace, &pub_inputs, &proof_options).unwrap();
    println!("  Time spent in proving: {:?} \n", timer.elapsed());

    assert!(verify_cairo_proof(&proof, &pub_inputs, &proof_options));
}
```

接下只需要分析`generate_prover_args`，因为后续的prove和verify是stark的工作，简单的略作分析就好了。

### generate_prover_args

整个看一下该函数

```rust
pub fn generate_prover_args(
    program_content: &[u8],
    layout: CairoLayout,
) -> Result<(TraceTable<Stark252PrimeField>, PublicInputs), Error> {
    let (register_states, memory, mut public_inputs) = run_program(None, layout, program_content)?;

    let main_trace = build_main_trace(&register_states, &memory, &mut public_inputs);

    Ok((main_trace, public_inputs))
}
```

程序输入的`program_content`实际只是对输入的json做一个byte处理，并没有改变任何数据结构，如果使用该代码，就会恢复原始的json格式数据。

```rust
println!("{}", String::from_utf8_lossy(&program_content));
```

进入到`run_program`函数中。该函数的作用是根据json文件数据运行整个程序，然后返回trace。输入参数中`entrypoint_function`表示要运行哪个加密函数，如果是None就运行main函数，之前的代码中可以看到，该参数的输入为None。

#### run_program

整体看一下`run_program`函数代码

```rust
pub fn run_program(
    entrypoint_function: Option<&str>,
    layout: CairoLayout,
    program_content: &[u8],
) -> Result<(RegisterStates, CairoMemory, PublicInputs), Error> {
    // default value for entrypoint is "main"
    let entrypoint = entrypoint_function.unwrap_or("main");

    let trace_enabled = true;
    let mut hint_executor = BuiltinHintProcessor::new_empty();
    let cairo_run_config = cairo_run::CairoRunConfig {
        entrypoint,
        trace_enabled,
        relocate_mem: true,
        layout: layout.as_str(),
        proof_mode: true,
        secure_run: None,
        disable_trace_padding: false,
    };

    let (runner, vm) =
        match cairo_run::cairo_run(program_content, &cairo_run_config, &mut hint_executor) {
            Ok(runner) => runner,
            Err(error) => {
                eprintln!("{error}");
                panic!();
            }
        };

    let relocated_trace = vm.get_relocated_trace().unwrap();

    let mut trace_vec = Vec::<u8>::new();
    let mut trace_writer = VecWriter::new(&mut trace_vec);
    trace_writer.write_encoded_trace(relocated_trace);

    let relocated_memory = &runner.relocated_memory;

    let mut memory_vec = Vec::<u8>::new();
    let mut memory_writer = VecWriter::new(&mut memory_vec);
    memory_writer.write_encoded_memory(relocated_memory);

    trace_writer.flush().unwrap();
    memory_writer.flush().unwrap();

    //TO DO: Better error handling
    let cairo_mem = CairoMemory::from_bytes_le(&memory_vec).unwrap();
    let register_states = RegisterStates::from_bytes_le(&trace_vec).unwrap();

    let vm_pub_inputs = runner.get_air_public_input(&vm).unwrap();

    let mut pub_memory: HashMap<Felt252, Felt252> = HashMap::new();
    vm_pub_inputs.public_memory.iter().for_each(|mem_cell| {
        let addr = Felt252::from(mem_cell.address as u64);
        let value = Felt252::from_hex_unchecked(&mem_cell.value.as_ref().unwrap().to_str_radix(16));
        pub_memory.insert(addr, value);
    });

    let mut memory_segments: HashMap<SegmentName, Segment> = HashMap::new();
    vm_pub_inputs.memory_segments.iter().for_each(|(k, v)| {
        memory_segments.insert(SegmentName::from(*k), Segment::from(v));
    });

    let num_steps = register_states.steps();
    let public_inputs = PublicInputs {
        pc_init: Felt252::from(register_states.rows[0].pc),
        ap_init: Felt252::from(register_states.rows[0].ap),
        fp_init: Felt252::from(register_states.rows[0].fp),
        pc_final: Felt252::from(register_states.rows[num_steps - 1].pc),
        ap_final: Felt252::from(register_states.rows[num_steps - 1].ap),
        range_check_min: Some(vm_pub_inputs.rc_min as u16),
        range_check_max: Some(vm_pub_inputs.rc_max as u16),
        memory_segments,
        public_memory: pub_memory,
        num_steps,
    };

    Ok((register_states, cairo_mem, public_inputs))
}
```

进入到`cairo_run`函数中，该函数不做分析（因为是cario-vm的）。

在`match cairo_run::cairo_run`之后相当于整个程序执行完，有了trace和vm，但是离最终的trace表生成还有一定距离。接下来首先要做的是`relocated_trace`。

之前生成的trace只是根据程序指令逐条生成一个trace，并不完全满足要求。所以要对之前的trace做一些特别处理，`relocated_trace`就是对整体做segment。之前程序`let relocated_trace = vm.get_relocated_trace().unwrap();`如果打印`relocated_trace`结果会是

```
[
    TraceEntry {
        pc: 1,
        ap: 14,
        fp: 14,
    },
    TraceEntry {
        pc: 3,
        ap: 14,
        fp: 14,
    },
    TraceEntry {
        pc: 7,
        ap: 16,
        fp: 16,
    },
    TraceEntry {
        pc: 9,
        ap: 17,
        fp: 16,
    },
    TraceEntry {
        pc: 11,
        ap: 17,
        fp: 16,
    },
    TraceEntry {
        pc: 5,
        ap: 17,
        fp: 14,
    },
    TraceEntry {
        pc: 5,
        ap: 17,
        fp: 14,
    },
    TraceEntry {
        pc: 5,
        ap: 17,
        fp: 14,
    },
]
```

随后`trace_writer.write_encoded_trace(relocated_trace);`将上述的内容按照`[ap, fp, pc]`的顺序写入到`trace_writer`中。

`relocated_memory`结构输出是这样

```
[
    None,
    Some(
        290341444919459839,
    ),
    Some(
        0,
    ),
    Some(
        1226245742482522112,
    ),
    Some(
        4,
    ),
    ...
```

类似的通过`memory_writer.write_encoded_memory(relocated_memory);`将数据写入，写入格式是`[i, data]`(none略过)，其中i表示内存索引，比如第一个数据是`[1, 290341444919459839]`(none跳过了，但是也要计入索引)

上述segment划分后，`ap，fp`和memory data划分之后也要保持一一对应的关系，因此`ap，fp`不能继续沿用original trace。变换方式是还是用segment那一套。比如原始的`ap`对应的是segment的2:3，那么随着重新排列，`ap`的位置也要调整。但是需要注意的是`relocated_trace`长度一般不会和`relocated_memory`长度一致，因为不是一条指令对应一个内存数据变化，很可能是一条指令会有多个内存数据变化。比如说`relocated_trace`长度只有8，对应的是8条指令，但是`relocated_memory`长度有17，说明内存数据存放了17个值。不过也可以看到`relocated_trace`中`ap`有17这个数说明确实还是有对应关系的，只不过不见得是完全一一对应。

接着`cairo_mem`是对之前的`relocated_memory`处理后格式化输出，`register_states`是对`relocated_trace`处理后格式化输出。

接下来会用到`vm_pub_inputs: PublicInput`，首先看一下`PublicInput`字段的定义

```rust
pub struct PublicInput<'a> {
    pub layout: &'a str,
    pub rc_min: isize,
    pub rc_max: isize,
    pub n_steps: usize,
    pub memory_segments: HashMap<&'a str, MemorySegmentAddresses>,
    pub public_memory: Vec<PublicMemoryEntry>,
    #[serde(rename = "dynamic_params")]
    layout_params: Option<&'a CairoLayout>,
}
```

说一下各个字段的含义：
layout：
**Role**: In Cairo, the "layout" determines the specific configuration of the memory segments, built-ins, and other resources used during program execution. Different layouts can optimize the VM's performance for various use cases, such as computation-heavy programs, programs with extensive memory usage, or those that require specific built-ins like range checks or bitwise operations. Common layouts might include "small", "dex", "recursive", etc.

The layout affects how the memory is organized and which built-in operations are available and optimized for the program. Choosing the correct layout is crucial for the program's efficiency and correctness.

`rc_min`, `rc_max`:
These fields represent the minimum and maximum values that the range check built-in can handle during the execution. They define the range within which the values should lie for the computation to be considered valid. If any value falls outside this range, it could signal an error or invalid computation.

n_steps:
- **Definition**: The `n_steps` field represents the number of steps (or instructions) that the Cairo VM will execute for a given program.
- **Role**: This is effectively the number of cycles or iterations the Cairo VM will perform to execute the program. In Cairo, a "step" refers to the execution of an instruction in the Cairo byte code. The total number of steps can be crucial for understanding the program's complexity and resource usage, as it can affect both the runtime and the proof generation time in zero-knowledge proofs.

layout_params:
- **Definition**: The `layout_params` field is an optional reference to a `CairoLayout`, which contains additional parameters specific to the chosen layout.

- **Role**: The `layout_params` provide specific configuration details for the selected layout. These parameters might include settings like memory segment sizes, the number of bits used in certain operations, or other layout-specific optimizations. They allow fine-tuning of the Cairo VM's behavior to better match the needs of the specific program being executed.
  The presence of `layout_params` makes the `layout` more flexible and adaptable to different use cases. By specifying these parameters, you can control various aspects of the VM's behavior, optimizing it for performance, memory usage, or other criteria depending on the application.

还是之前的例子，`vm_pub_inputs`输出是：

```rust
Public Inputs: PublicInput { layout: "plain", rc_min: 32766, rc_max: 32769, n_steps: 8, memory_segments: {"program": MemorySegmentAddresses { begin_addr: 1, stop_ptr: 5 }, "execution": MemorySegmentAddresses { begin_addr: 14, stop_ptr: 17 }}, public_memory: [PublicMemoryEntry { address: 1, value: Some(290341444919459839), page: 0 }, PublicMemoryEntry { address: 2, value: Some(0), page: 0 }, PublicMemoryEntry { address: 3, value: Some(1226245742482522112), page: 0 }, PublicMemoryEntry { address: 4, value: Some(4), page: 0 }, PublicMemoryEntry { address: 5, value: Some(74168662805676031), page: 0 }, PublicMemoryEntry { address: 6, value: Some(0), page: 0 }, PublicMemoryEntry { address: 7, value: Some(5189976364521848832), page: 0 }, PublicMemoryEntry { address: 8, value: Some(3), page: 0 }, PublicMemoryEntry { address: 9, value: Some(4613515612218425343), page: 0 }, PublicMemoryEntry { address: 10, value: Some(3), page: 0 }, PublicMemoryEntry { address: 11, value: Some(2345108766317314046), page: 0 }, PublicMemoryEntry { address: 12, value: Some(14), page: 0 }, PublicMemoryEntry { address: 13, value: Some(0), page: 0 }], layout_params: None }
```

说一下一些字段的含义。`begin_addr`某一个memory segment的start memory的地址，`stop_ptr`(stop pointer)表示某一个memory segment的stop memory的地址。

`page`（A division within the virtual memory space used to organize memory entries.）是用于做内存分区和管理。

随后代码将之前的`pub_memory`数据按照hash的方式存储在`pub_memory`中。

`memory_segments`同样也是hash处理，形成"program" -> MemorySegmentAddresses 映射。

`num_steps`就是`register_states`的row长度，在这个例子中是8，最后将这些数据输出。至此`run_program`结束，接下来进入到`build_main_trace`函数中。

#### build_main_trace

看一下`build_main_trace`整体代码，这部分内容有[参考文档](https://lambdaclass.github.io/lambdaworks/starks/cairo_trace_descriptive.html)

```rust
pub fn build_main_trace(
    register_states: &RegisterStates,
    memory: &CairoMemory,
    public_input: &mut PublicInputs,
) -> CairoTraceTable {
    let mut main_trace = build_cairo_execution_trace(register_states, memory);

    let mut address_cols =
        main_trace.merge_columns(&[FRAME_PC, FRAME_DST_ADDR, FRAME_OP0_ADDR, FRAME_OP1_ADDR]);

    address_cols.sort_by_key(|x| x.representative());

    let (rc_holes, rc_min, rc_max) = get_rc_holes(&main_trace, &[OFF_DST, OFF_OP0, OFF_OP1]);

    // this will avaluate to true if the public inputs weren't obtained from the run_program() function
    if public_input.range_check_min.is_none() && public_input.range_check_max.is_none() {
        public_input.range_check_min = Some(rc_min);
        public_input.range_check_max = Some(rc_max);
    }
    fill_rc_holes(&mut main_trace, &rc_holes);

    let memory_holes = get_memory_holes(&address_cols, &public_input.public_memory);

    if !memory_holes.is_empty() {
        fill_memory_holes(&mut main_trace, &memory_holes);
    }

    add_pub_memory_dummy_accesses(
        &mut main_trace,
        public_input.public_memory.len(),
        memory_holes.len(),
    );

    let trace_len_next_power_of_two = main_trace.n_rows().next_power_of_two();
    let padding_len = trace_len_next_power_of_two - main_trace.n_rows();
    main_trace.pad_with_last_row(padding_len);

    main_trace
}
```

首先进入到`build_cairo_execution_trace`函数中。

```rust
pub fn build_cairo_execution_trace(
    register_states: &RegisterStates,
    memory: &CairoMemory,
) -> CairoTraceTable {
    let n_steps = register_states.steps();

    // Instruction flags and offsets are decoded from the raw instructions and represented
    // by the CairoInstructionFlags and InstructionOffsets as an intermediate representation
    let (flags, offsets): (Vec<CairoInstructionFlags>, Vec<InstructionOffsets>) = register_states
        .flags_and_offsets(memory)
        .unwrap()
        .into_iter()
        .unzip();

    // dst, op0, op1 and res are computed from flags and offsets
    let (dst_addrs, mut dsts): (Vec<Felt252>, Vec<Felt252>) =
        compute_dst(&flags, &offsets, register_states, memory);
    let (op0_addrs, mut op0s): (Vec<Felt252>, Vec<Felt252>) =
        compute_op0(&flags, &offsets, register_states, memory);
    let (op1_addrs, op1s): (Vec<Felt252>, Vec<Felt252>) =
        compute_op1(&flags, &offsets, register_states, memory, &op0s);
    let mut res = compute_res(&flags, &op0s, &op1s, &dsts);

    // In some cases op0, dst or res may need to be updated from the already calculated values
    update_values(&flags, register_states, &mut op0s, &mut dsts, &mut res);

    // Flags and offsets are transformed to a bit representation. This is needed since
    // the flag constraints of the Cairo AIR are defined over bit representations of these
    let trace_repr_flags: Vec<[Felt252; 16]> = flags
        .iter()
        .map(CairoInstructionFlags::to_trace_representation)
        .collect();
    let trace_repr_offsets: Vec<[Felt252; 3]> = offsets
        .iter()
        .map(InstructionOffsets::to_trace_representation)
        .collect();

    // ap, fp, pc and instruction columns are computed
    let aps: Vec<Felt252> = register_states
        .rows
        .iter()
        .map(|t| Felt252::from(t.ap))
        .collect();
    let fps: Vec<Felt252> = register_states
        .rows
        .iter()
        .map(|t| Felt252::from(t.fp))
        .collect();
    let pcs: Vec<Felt252> = register_states
        .rows
        .iter()
        .map(|t| Felt252::from(t.pc))
        .collect();
    let instructions: Vec<Felt252> = register_states
        .rows
        .iter()
        .map(|t| *memory.get(&t.pc).unwrap())
        .collect();

    // t0, t1 and mul derived values are constructed. For details reFelt252r to
    // section 9.1 of the Cairo whitepaper
    let two = Felt252::from(2);
    let t0: Vec<Felt252> = trace_repr_flags
        .iter()
        .zip(&dsts)
        .map(|(repr_flags, dst)| (repr_flags[9] - two * repr_flags[10]) * dst)
        .collect();
    let t1: Vec<Felt252> = t0.iter().zip(&res).map(|(t, r)| t * r).collect();
    let mul: Vec<Felt252> = op0s.iter().zip(&op1s).map(|(op0, op1)| op0 * op1).collect();

    // A structure change of the flags and offsets representations to fit into the arguments
    // expected by the TraceTable constructor. A vector of columns of the representations
    // is obtained from the rows representation.
    let trace_repr_flags = rows_to_cols(&trace_repr_flags);
    let trace_repr_offsets = rows_to_cols(&trace_repr_offsets);

    let extra_addrs = vec![Felt252::zero(); n_steps];
    let extra_vals = extra_addrs.clone();
    let rc_holes = extra_addrs.clone();

    // Build Cairo trace columns to instantiate TraceTable struct as defined in the trace layout
    let mut trace_cols: Vec<Vec<Felt252>> = Vec::new();
    (0..trace_repr_flags.len()).for_each(|n| trace_cols.push(trace_repr_flags[n].clone()));
    trace_cols.push(res);
    trace_cols.push(aps);
    trace_cols.push(fps);
    trace_cols.push(pcs);
    trace_cols.push(dst_addrs);
    trace_cols.push(op0_addrs);
    trace_cols.push(op1_addrs);
    trace_cols.push(instructions);
    trace_cols.push(dsts);
    trace_cols.push(op0s);
    trace_cols.push(op1s);
    (0..trace_repr_offsets.len()).for_each(|n| trace_cols.push(trace_repr_offsets[n].clone()));
    trace_cols.push(t0);
    trace_cols.push(t1);
    trace_cols.push(mul);
    trace_cols.push(extra_addrs);
    trace_cols.push(extra_vals);
    trace_cols.push(rc_holes);

    TraceTable::from_columns_main(trace_cols, 1)
}
```

该函数就是根据之前说的instruction structure来生成trace。其中`trace_repr_flags`是flags所组成的二进制16位数组。对应的就是instruction structure的最后面一列，包含`dst_reg`, `op0_reg`, `op1_src`, `res_logic`, `pc_update`, `ap_update` and `opcode` flags, as well as the zero flag。`trace_repr_offsets`是3个数值所组成的列表，3个数值分别是`[off_dst, off_op0, off_op1]`，通过这两个数据就可以组合出来整个table了。

上述的这两个数据生成来自之前的memory data以及`register_states`，通过一些运算可以生成整个instruction structure table。按照之前的例子，`register_states`的row是8，同样的生成的`trace_repr_flags`以及`trace_repr_offsets`的row也是8。

再之后是`ap, fp, pc and instruction columns are computed`，`ap, fp, pc`根据`register_states`还原即可，instruction是使用pc从memory的index去查value，该值是instruction的值。同样的上述4个row也是8。

接下来是 `t0`的计算，根据之前论文9.4以及解读部分有 $\tilde f_i - 2 \tilde f_{i+1} = f_i$ 所以如果想获得`pc_jnz`(或者是0或者是1) 只需要 `repr_flags[9] - two * repr_flags[10]` 根据9.5公式可知`(repr_flags[9] - two * repr_flags[10]) * dst`即可计算`t0`。

根据9.5论文中的内容，当`pc_jnz`的值有效时（值为1），此时的res的结果应该是unused or undefined ，也就是值是无关不重要的。此时没有必要再浪费一列来专门保存v，不如就在res无效时，将v的内容保存在res列中，利用一下res的空间，避免再新开一列。反应在代码中就是`t1`的计算是通过 `t * r` 实现的。

`mul`的作用就是计算`op0`和`op1`这两个寄存器的乘积，`op0 * op1`。

随后`let trace_repr_flags = rows_to_cols(&trace_repr_flags);`将行和列的顺序调换，之前是16行8列，转换之后是16列8行，也就是每一个flag都单独成一列，`trace_repr_offsets`也是同样道理。

`rc_holes` （rc: range check）来自论文9.9，目前先是全部一列数据赋值为0，将来会通过`fill_rc_holes`函数把一个排列好的数据去填写进去，如果排列好的数据内容值不够填满全部都是0的列，就会重复填入排列好数据的最后一个元素，直到填满整列为止。

最后把上述计算结果存储在`trace_cols`中组成一个表。然后`TraceTable::from_columns_main(trace_cols, 1)`是将二维数组转换成1维。转换方式是按行存储，也就是把第一行数组存储完之后存第二行，所有数据全部都存在同一个数组中。比如之前的`trace_cols`是36列数据，每个数据是8行。新的数据结构是每一列36个数据，然后8行，不过是存在一列里面，所以一共是288个数据。

回到`build_main_trace`函数中，`merge_columns`实际上就是根据想要选择的column索引，将这些column组成一列然后返回。比如`&[FRAME_PC, FRAME_DST_ADDR]`就获取该2列的数据然后组成一个列并返回。

对`address_cols`使用`merge_columns`之后对其值按照从小到大的顺序重新排序。

`get_rc_holes`是获得一个排列数据，然后将这个排列数据在`fill_rc_holes`函数中填入到main_trace的`RC_HOLES`列，将原来的0值替换掉。如果排列好的数据内容值不够填满全部都是0的列，就会重复填入排列好数据的最后一个元素，直到填满整列为止。
