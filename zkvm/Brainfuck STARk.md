

一个新的概念是 **Consistency Constraint** ，该约束是保证内存中的`cell` 值不会发生改变，除非instruction更改了cell的结果，也就是说不管Cycle执行到哪里，那么Cell中的值不应该随意发生变化，除非instruction将其更改。举例来说，之前例子中Cycle 2的`Cell 0`和Cycle 4的`Cell 0`里面的值应该是一样的。

具体一致性的保证方式是通过 `original * inverse - 1 ?= 0` 来实现的。如果只改original但是不改inverse是无法使得上述约束成立。但是具体如何防止使用欺骗的方式同时改original和inverse还需要看代码才能知道。

**Transaction Constraint**

式子中P1, P2, P3多项式约束是来保证根据指令执行程序时状态（比如内存值，指针位置等）正确发生变更。每一个P表示一个不同的part。brain fuck一共有8条指令，每一个指令都最多可以对应3条状态变更约束。（详见链接中的表）这些p的约束像是if else condition（互斥关系），所以不会同时生效，类似一种条件判断，在不同条件下会生效其中一个，因此总共对8个指令来说是一张表，而不是3张。



# new

- For both sorted columns, compute the column of consecutive differences. A simple transition constraint shows that the column of consecutive differences was computed integrally. The first element of the consecutive difference column should be the same as the first element of the sorted column, and this fact can be established with a boundary constraint.

其中的difference column指的是和原始的column内部两两元素不同组成的新的column，什么意思比如说原始的column是`[3, 5, 8, 10]`，然后第一个元素保持不变，5和3之间的不同是2，所以第二个元素是2，8和5之间是3，第三个元素是3。因此difference column是`[3, 2, 3, 2]`
- **Compute Consecutive Differences**: For a sorted column, create a new column where each element is the difference between consecutive elements in the original sorted column.
- **Transition Constraint**: Ensure that the differences were computed correctly.
- **Boundary Constraint**: Verify that the first element in the differences column matches the first element in the sorted column.

# VM

进入到`vm.py`，从test代码可知首先进入到的是compile函数。该函数的作用是将原市的brain fuck代码转换成机器码（unicode int in finite field），重点看一下两个循环指令

```python
# parser
program = []
# keeps track of loop beginnings while (potentially nested) loops are being compiled
stack = []
for symbol in brainfuck_code:
	program += [F(symbol)]
	# to allow skipping a loop and jumping back to the loop's beginning, the respective start and end positions
	# are recorded in the program. For example, the (nonsensical) program `+[>+<-]+` would be `+[9>+<-]3+`.
	if symbol == '[':
		# placeholder for position of loop's end, to be filled in once position is known
		program += [zero]
		stack += [len(program) - 1]
	elif symbol == ']':
		# record loop's end
		program += [BaseFieldElement(stack[-1] + 1, field)]
		# record loop's beginning
		program[stack[-1]] = BaseFieldElement(len(program), field)
		stack = stack[:-1]
```

brain fuck vm 中关于循环指令规则是
- `[` begin loop: 如果当前memory cell的值是0，那么指令指针将会跳转到`]`之后的执行程序。如果不是0，就继续执行`[`后面的程序。
- `]` end loop: 如果当前memory cell的值是0，指令指针继续执行`]`后面的程序。如果不是0，跳转到`[`之后的位置执行程序。

每次遇到`[`时，程序指令都会在往前加一个0，`program += [zero]` 因此在[processorTable](https://aszepieniec.github.io/stark-brainfuck/arithmetization)中才会有 $(ip^* - ip - 2) \cdot mv$ 是减2而不是减1。`stack += [len(program) - 1]`用于标识`[`在program程序中的位置，将来会被`]`修改，不会一直是0，其目的是为了确定`]`位置，因为如果begin loop的值是0的话，需要跳转到`]`之后的位置，所以需要做记录，将来会通过`program[stack[-1]] = BaseFieldElement(len(program), field)`将这个位置修正。在`[`指令中，还需要一个位置用来记录`[`之后的指令位置，因为如果值不是0的话，需要跳转到`[`之后的位置。`[`本身有一个值，然后还要在用一个位置来保存`]`之后的指令位置，所以`[`之后的指令还要再加1，于是有`program += [BaseFieldElement(stack[-1] + 1, field)]`。最后一层循环结束，要把最内存的循环stack扔掉，于是有`stack = stack[:-1]`。

有了`program`之后接下来进入`run`函数。该函数就是具体执行brain fuck vm的计算流程使用instruction pointer来逐条执行program以及memory pointer来变更memory的值。

# Tables

5个表分别是processor table，instruction table，memory table，input table，output table。相比于之前的单纯执行vm，要想勾连起这5个表，instruction pointer和memory pointer是不够的，需要使用一个新的数据结构`Register`，具体定义可见[文章头部](https://aszepieniec.github.io/stark-brainfuck/arithmetization)，Register初始值都设置为`0`。

接下来进入`simulate`看一下表如何生成，该函数和同之前的`run`函数类似，所不同的是会生成5个表。表的目的是用来确定约束关系，确保程序确实是按要求来生成的。

一个实际的程序是如何转换成多项式约束的呢？这里举一个例子。

```
++>+
```

这个程序首先在内存`cell 0`处将值加2（++），然后内存指针前进1（>），接着内存`cell 1`处值加1（+），可以写成表格的形式


| Cycle             | Memory Pointer | Cell 0 | Cell 1 |
| ----------------- | -------------- | ------ | ------ |
| 0 (init)          | 0              | 0      | 0      |
| 1 (first execute) | 0              | 1      | 0      |
| 2                 | 0              | 2      | 0      |
| 3                 | 1              | 2      | 0      |
| 4                 | 1              | 2      | 1      |

这样就可以将一个实际的程序转换成多项式了。Cycle这个列称之为**Processor Table** 。

`processor_matrix`就是将上述register按照[约束规则](https://neptune.cash/learn/brainfuck-tutorial/)将值填到表里。

`instruction_matrix`是将`processor_matrix`的部分值提取出来

```python
instruction_matrix += [[register.instruction_pointer,
						register.current_instruction,
						register.next_instruction]]

# sort by instruction address
instruction_matrix.sort(key=lambda row: row[0].value)
```

为什么要把关于instruction的部分值提取出来，而且还做一个排序，之前的`processor_matrix`不够吗？在[该文](https://aszepieniec.github.io/stark-brainfuck/engine) Table部分有讲，我们需要保证如果`instruction_pointer`不变，那么`current_instruction`和`next_instruction`也不应该发生变化。也就是说，通过排序后，比如有两行的instruction pointer是一样的，那么前一行的`current_instruction`与`next_instruction`同后一行的`current_instruction`与`next_instruction`值应该是一样的。而如果不对`instruction_pointer`排序的话，这种两行一致性（consistency）约束关系就无法建立（散乱分布在不同行的`ci`和`ni`建立一致性是比较困难的）。

接下来还有一个`memory_matrix`。

```python
memory_matrix = MemoryTable.derive_matrix(processor_matrix)

@staticmethod
def derive_matrix(processor_matrix):
	zero = processor_matrix[0][ProcessorTable.cycle].field.zero()
	one = processor_matrix[0][ProcessorTable.cycle].field.one()

	# copy unpadded rows and sort
	matrix = [[pt[ProcessorTable.cycle], pt[ProcessorTable.memory_pointer],
			   pt[ProcessorTable.memory_value], zero] for pt in processor_matrix if not pt[ProcessorTable.current_instruction].is_zero()]
	matrix.sort(key=lambda mt: mt[MemoryTable.memory_pointer].value)

	# insert dummy rows for smooth clk jumps
	i = 0
	while i < len(matrix)-1:
		if matrix[i][MemoryTable.memory_pointer] == matrix[i+1][MemoryTable.memory_pointer] and matrix[i+1][MemoryTable.cycle] != matrix[i][MemoryTable.cycle] + one:
			matrix.insert(i+1, [matrix[i][MemoryTable.cycle] + one, matrix[i]
						  [MemoryTable.memory_pointer], matrix[i][MemoryTable.memory_value], one])
		i += 1

	return matrix
```

memory表的设计之前提到的Table文章有详细讲解为什么要单独再拆出来一个memory table。简单来说，有这样一个场景约束，就是比如`>>>`跳转到别的memory位置，然后有`<<<`再跳回来，跳回来之后对于相同的memory pointer其memory value应该是一样的（值的修改需要再来新的instruction比如`+`），这个约束关系之前的processor table没有提供，所以需要新加这个memory表来提供约束。但是同instruction表一样，散落在各个地方难以直接建立约束。既然这个约束是基于memory pointer的，那么就对memory pointer去排序。

如果memory pointer一致的话，上下两行cycle number的差值居然超过1，就说明第二行一定是经过多轮操作然后重新回到memory pointer的位置上的，那么该行的memory value应该要和上一行的一致。如果cycle number差值是1的话，那么这个新行可能是执行类似`+`操作，虽然两行memory pointer一致，不过memory value可以不一致，不需要约束。

dummy是为了说上下两行差值大于1，需要做上下两行的想等性约束（所以最后给个1，说明该约束成立），但是这种插入新行的办法是否过于的蠢了。将来我在看看别的实现是否有更合理的方式去实现上述约束。