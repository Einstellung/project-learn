### Zk  Wordle 介绍

zk wordle是使用零知识证明的办法构建wordle的程序。wordle是一个小游戏，游戏规则是用户有6次机会输出5个字母排列组成的单词，后台会将用户输入的单词和正确的单词进行匹配。如果输入的字母位置和正确单词的位置相同，那么就会给出绿色标识符，如果用户输入的单词字母在正确单词的某个字母匹配，但是位置可能不一致，这时候就会给出黄色标识符。游戏的胜利机制就是结合之前的绿色和黄色标识提示，在6次机会以内找到正确的单词。

<div align="center">
<img src="https://user-images.githubusercontent.com/6984346/178630626-65108409-9fbf-4f08-bca6-66b4fa426fff.png"  height = "600" alt="图片名称" align=center />
</div>

假如说你成功找到了某个正确的单词，想向你的朋友炫耀一下。你的朋友可能会对此提出质疑。这个时候你为了打消他的质疑就需要向他证明你确实找到了某个正确单词。最简单的办法就是你直接告诉他正确的单词是什么，这样朋友就可以用这个你说的单词去实际验证一下是否真的正确。

但是这样一来，你相当于泄密了，你的朋友可能之前已经绞尽脑汁尝试了很多次都找不到正确的单词到底是什么，你告诉他这样他知道了，他就可以再别的朋友炫耀说自己找到正确单词（虽然实际上是你告诉他的，但他可能不会告诉别人）。那么有没有什么办法你在不泄露正确单词的情况下依旧向他证明你确实知道正确的单词是什么呢？

答案就是使用零知识证明这项技术。本文接下来就会具体介绍该如何设计和构建这样的证明。现在网络上已经有很多不同版本的使用零知识证明构建zk wordle的项目，接下来我要分析的是使用Halo2设计构建的项目，该项目地址在：https://github.com/nalinbhardwaj/zordle

### 数据处理

`dict.rs`文件里面包含一个非常巨大的数组，里面是经过处理的数据，数据源自`dict.json`文件。将5个字母的单词经过自定义的hash运算之后转成数字。方便将来在约束电路中进行约束，来保证用户输入的5个字母能够组成单词。自定义hash运算格式如下：
```rust
pub fn word_to_chars(word: &str) -> Vec<u64> {
    let mut res = vec![];
    for c in word.chars() {
        res.push((c as u64) - ('a' as u64) + 1);
    }
    res
}

pub fn word_to_polyhash(word: &str) -> u64 {
    let chars = word_to_chars(word);
    let mut hash = 0;
    for c in chars {
        hash = hash * BASE;
        hash += c;
    }

    hash
}
```

我们还需要用到`compute_diff`函数，他的主要作用是比较输入的单词和正确的单词之间的差异（这些单词都会经过`word_to_char`处理，所以实际上是比较数字之间的差异），然后放进数组里面。举一个例子，比如正确的单词是“fluff"，用户输入的单词是“fault”，那么"fluff"经过`word_to_char`处理之后是[6,12,21,6,6]，“fault”是[6,1,21,12,20]，`compute_diff`输出会是[[1,0,1,0,0], [1,0,1,1,0]\]
该数组的第一项是`green`项，也就是字母正确同时位置正确，用1表示。其余用0。数组第二项是`yellow`项，1表示该字母在正确单词的字母中，不关心位置是否正确。0表示该字母在正确的单词中未曾出现。

下面简单介绍一下整个数据处理流程。
首先用户有6次机会输入单词，然后用上面介绍的方法，把单词转成hash数组和`word_to_char`数组，再同真正正确的单词数组`final_char`做比较，用于生成green和yellow项（使用`compute_diff`），这两项数据最终会输入到instance中。用于circuit电路的`word_diffs_green`和`word_diffs_yellow`数据与`green`和`yellow`项类似，只不过没有经过0和1的二值化处理，还保留原始的结果。

### 约束电路

接下来是Zk Wordle的约束电路设计，主要内容在`wordle.rs`文件中。我们首先来看一下整体约束电路结构（我假设正确的单词是“fluff”，而用户输入的单词是"fault"）。

![circuit](https://github.com/Einstellung/project-learn/assets/26652483/5f96ae5a-2d2e-439b-8b5f-2e2562a5e0f3)

图中的Advice其实是11列，不过为了图表不过于太大便于展示，我将原本5列的char折叠只选取第一列和最后一列，color_is_zero列也同样如此。图中类似big或者inv不是表示cell中填入的是字母，而是真实数据，因为数值比较大，完整展示会导致图表比较难以展示，所以用big或者inv来代替。

接下来具体分析一下图表。其中Instance部分比较容易，我们在前一部分数据处理中已经做过分析。接下来分析Advice部分。

我们来横向的分析每一个row，首先是第零行`words`部分。poly_word部分是使用自定义的hash算法来对输入的单词("fault")处理之后的结果。后面是经过`word_to_char` 处理过后的单词数值表示。

第一行`final_words`填入正确单词的数值表示。

第二行 `diff_g`是将之前数据处理部分的`word_diffs_green`数据填入，第三行`diff_y`也是同样的，将`word_diffs_yellow`数据填入。

第四行`diff_g_is_zero`和第五行`diff_y_is_zero`是对`word_diffs_green`和`word_diffs_yellow`数据做二值化处理，然后填入。最终的结果应该要和instance中的`green`和`yellow`项一致。

第六行`color_green`和第七行`color_yellow`是将Instance列的`green`和`yellow`分别填入。

Advice列的`color_is_zero`项分别来自`diff_g`和`diff_y`的数据做inv处理，至于为什么数据会填在`diff_g_is_zero`和`diff_y_is_zero`行是来自如下代码设置：
```rust
diffs_green_is_zero_chips[i].assign(&mut region, 4, diffs_green[i])?;
diffs_yellow_is_zero_chips[i].assign(&mut region, 5, diffs_yellow[i])?;
```

下面我们看一下整个电路的约束是如何设计的。

首先是查表约束，用于约束输入的单词是有效的单词而不是乱输入的字母。该约束表在`table`中，是一个非常巨大的表。
```rust
let table = DictTableConfig::configure(meta);

meta.lookup(|meta| {
	let q_lookup = meta.query_selector(q_input);
	let poly_word = meta.query_advice(poly_word, Rotation::cur());

	vec![(q_lookup * poly_word, table.value)] // check if q_lookup * value is in the table.
});
```

接下来是“range check”约束，用于约束输入的word确实是26个英文字母输入，因为``
`word_to_char`计算会+1，所以实际上是从1开始遍历。
```rust
meta.create_gate("character range check", |meta| {
	let q = meta.query_selector(q_input);
	let mut constraints = vec![];
	for idx in 0..WORD_LEN {
		let value = meta.query_advice(chars[idx], Rotation::cur());

		let range_check = |range: usize, value: Expression<F>| {
			assert!(range > 0);
			(1..range).fold(Expression::Constant(F::ONE), |expr, i| {
				expr * (Expression::Constant(F::from(i as u64)) - value.clone())
			})
		};

		constraints.push(q.clone() * range_check(28, value.clone()));
	}
	constraints
});
```
原始代码处使用`.fold(value.clone()...)`，但我认为不太妥当，因为如果当value值为0的话，这个约束总是满足就达不到约束的效果，所以我将其修改为`Expression::Constant(F::ONE)`。

“poly hashing check”约束用于计算`poly_hash`和输入的words结果的一致性。

“diff_g checker”约束用于确保`diff_g`行的数据确实是由用户输入的字母和真实结果相减的差。

“diff_y checker”的检查和“diff_g checker”十分相似，但因为是检查yellow项，要比green检查略微复杂一点。要将每个字母都和final_char做差，所以是嵌套迭代。如果差为0，那么就说明找到这个字母，因为只需要找到一个就可以了，所以是用乘积关系，保证总体乘积是0。直观解释可能略微苍白，代码比较清晰的展示这一点。
```rust
meta.create_gate("diff_y checker", |meta| {
	let q = meta.query_selector(q_diff_y);
	let mut constraints = vec![];
	for i in 0..WORD_LEN {
		let char = meta.query_advice(chars[i], Rotation(-3));
		let diff_y = meta.query_advice(chars[i], Rotation::cur());

		let yellow_check = {
			(0..WORD_LEN).fold(Expression::Constant(F::ONE), |expr, i| {
				let final_char = meta.query_advice(chars[i], Rotation(-2));
				expr * (char.clone() - final_char)
			})
		};
		constraints.push(q.clone() * (yellow_check - diff_y));
	}

	constraints
});
```

"diff_color_is_zero checker"约束稍微复杂一点，一般的create_gate一次定义的时候只会有一个约束项起作用，但是这个create_gate定义的时候同时有多个约束项会起作用。
```rust
self.q_diff_green_is_zero.enable(&mut region, 4)?;
self.q_color_is_zero.enable(&mut region, 4)?;
self.q_diff_yellow_is_zero.enable(&mut region, 5)?;
self.q_color_is_zero.enable(&mut region, 5)?;

meta.create_gate("diff_color_is_zero checker", |meta| {
	let q_green = meta.query_selector(q_diff_green_is_zero);
	let q_yellow = meta.query_selector(q_diff_yellow_is_zero);
	let q_color_is_zero = meta.query_selector(q_color_is_zero);
	let mut constraints = vec![];

	for i in 0..WORD_LEN {
		let diff_color_is_zero = meta.query_advice(chars[i], Rotation::cur());

		constraints.push(q_color_is_zero.clone() * (diff_color_is_zero - (q_green.clone() * diffs_green_is_zero[i].expr() + q_yellow.clone() * diffs_yellow_is_zero[i].expr())));
	}

	constraints
});
```
仔细看代码，首先要解决的问题是`Rotation::cur()`到底在哪一行的问题，特别是这些约束项并不完全在同一行起作用。

我刚研究这段代码的时候也烦糊涂，事实上答案是`Rotation::cur()`既在第四行，又在第五行。这是一种电路设计技巧，如果你对此觉得疑惑，可以想象我实际写了2个`create_gate`，将其拆分的话是不是就不存在`Rotation::cur()`到底在哪一行的问题？那么上面这段代码只是把两个近乎一样的`create_gate`放在一起了而已。

为什么可以放在一起呢？因为`q_selector`这样的约束项要么值是1要么值是0，如果这一项的值为0，那么这个约束本身也不起作用。当`Rotation::cur()`在第四行的时候，第五行的`q_yellow`项就是0，那么此时整个约束项就简化成了
```rust
q_color_is_zero.clone() * (diff_color_is_zero - q_green.clone() * diffs_green_is_zero[i].expr())
```
本来结果也就没有第五行任何事情，所以并不需要担心各个约束项不在同一行。

约束式中`diffs_green_is_zero[i].expr()`是`is_zero`约束，该约束实际上是值的约束。value * value_inv == 1，那么如果把value_inv的值确定了，value的值就应该确定。也就是`diff_g`和`diff_y`的值和`color_is_zero`行对应的值应该有对应关系。

该约束的主要作用是确保`diff_g_is_zero`和`diff_y_is_zero`确实是由`diff_g`和`diff_y`的二值化计算二来。我以`diff_g_is_zero`举例来说明。如果diff_g的结果是0，那么1 - value * value_inv的结果是1，这个时候`diff_g_is_zero`的结果就应该是1。如果diff_g的结果不是0，那么1 - value * value_inv的结果是0。这个时候`diff_g_is_zero`的结果就应该是1。

“color check”也是有两行约束，分析原理同"diff_color_is_zero checker"一样，这里就不做展开说明了。该约束的主要作用是确保`color`和`diff_color`的值相反，和`diff_color_is_zero`值相同。

以上是对约束的全部介绍，也是Zk wordle的核心部分。关于WebAssembly部分，这里就不做展开介绍了。需要注意的是因为该项目使用的是nightly-2022-04-07 toolchain构建，使用最新的wasm无法打包rust代码，需要使用更低版本的wasm，我尝试使用0.10.3构建成功。但是该版本打包出来的js代码结构和最新版本的wasm代码已经有了较大差异，可能不太有足够的学习参考价值。如果对于Rust构建wasm感兴趣的可以参考这篇教程学习：https://rustwasm.github.io/docs/book/introduction.html
