***

name: "slint-ui"
description: "Develop GUIs with the Slint UI language (.slint files, Rust/C++/JS/Python integration). Invoke when user writes Slint code, mentions Slint, or asks to build desktop/embedded/mobile UIs with Slint. 使用 Slint 编写 UI、集成 Rust/C++/Node.js/Python 时使用本技能。"
---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------

# Slint UI 开发指南

整理自 Slint 官方文档 (<https://docs.slint.dev/latest/docs/slint/>)。

**核心理念:** UI 用 `.slint` 声明式语言描述; 业务逻辑用宿主语言 (Rust、C++、JavaScript、Python) 实现。Slint 是静态类型且响应式的 — 绑定自动跟踪依赖, 依赖变化时自动重新求值。

## 适用场景

- 编写或修改 `.slint` 文件

- 将 Slint UI 与 Rust、C++、Node.js、Python 代码集成

- 为 Slint UI 选择元素、部件或布局

- 调试 Slint 属性、回调、状态问题

***

## 1. .slint 文件

```slint
// 从标准部件库导入
import { Button, VerticalBox } from "std-widgets.slint";

// 组件是基本构建单元; `export` 使其可被其他文件和宿主语言使用
export component MainWindow inherits Window {
    width: 326px;
    height: 326px;
    title: "My App";
}
```

关键规则:

- 文件扩展名 `.slint`; 语法: `keyword value;` (属性绑定后要分号, `{ ... }` 块后不需要)。

- 组件通过 `inherits` 继承另一个组件 (元素) 来扩展。

- 用 `id := ElementName { ... }` 为元素命名以便其他地方引用 (`root` 隐式指顶层元素; 作用域访问用 `root.x`、`self.x`、`parent.x`)。

- 导入: `import { Name } from "std-widgets.slint";` 或自定义文件: `import { MyComponent } from "other-file.slint";`。

## 2. 属性

```slint
export component Button {
    in property <string> text;          // 输入: 由组件的使用者设置
    out property <bool> pressed;        // 输出: 仅组件内部可设置 (外部只读)
    in-out property <bool> checked;    // 所有人可读写
    private property <bool> has-mouse;  // 内部私有 (默认限定符)
}
```

- 绑定: 表达式 `width: 42px;` 或代码块 `width: { 42px }`, 二者都是响应式的。

- 默认值 = 类型的默认值 (false、0、""、transparent 等)。

- 属性名用 kebab-case: `has-hover`、`is-pressed`。

- **changed 回调**: `changed text => { ... }` — 属性值变化时触发 (排队执行, 每个事件循环周期至多一次)。能用声明式绑定就不要用 changed 回调:

  - 差: `changed bar => { foo = bar + 1; }`

  - 好: `foo: bar + 1;`

- **双向绑定**: `a <=> b;` 让两个同类型属性始终同步 (值相同, 类型可省略自动推断)。常用于表单输入与模型字段同步: `input.text <=> model.name;`。也用于回调别名: `callback clicked <=> area.clicked;`。

## 3. 原始类型

- `bool` — 默认值 `false`。字面量: `true` / `false`。

- `int` — 默认值 `0`。字面量: `42`。float 转 int 会截断 (45.8 变 45)。

- `float` — 默认值 `0`。字面量: `3.14`、`30%` (等于 0.30)。

- `string` — 默认值 `""`。字面量: `"hello"`, 转义 `\" \\ \n \u{x} \{expr}`。常用: `.is-empty`、`.character-count`、`.to-lowercase()`、`.to-uppercase()`。

- `length` — 默认值 `0px`。字面量: `1px`、`1pt`、`1in`、`1mm`、`1cm`。用于 x/y/width/height。

- `physical-length` — 默认值 `0phx`。字面量: `100phx`。物理像素。

- `percent` — 默认值 `0%`。带 `%` 后缀的 32 位浮点数; 分配给该类型的字面量必须带 `%` 后缀 (如 `50%`)。

- `relative-font-size` — 默认值 `0rem`。相对字号因子, 与 `Window.default-font-size` 相乘后转成 length。

- `duration` — 默认值 `0ms`。字面量: `100ms`、`2s`。

- `angle` — 默认值 `0deg`。字面量: `90deg`、`1.2rad`、`0.25turn`。

- `color` — 默认值 `transparent`。字面量: `#RRGGBBAA`、`#RGB`、CSS 颜色名 (`red`、`blue`)。

- `brush` — 默认值 `transparent`。颜色或渐变, 如 `@linear-gradient(40deg, ...)`。

- `image` — 默认为空图。用 `@image-url("path.png")`。`.width` / `.height` 取尺寸; 支持 SVG、PNG、JPEG、GIF、WebP 等格式。

- `easing` — 默认值 `linear`。可选: `ease-in`、`ease-out`、`ease-in-out`、`ease-in-bounce`、`cubic-bezier(a,b,c,d)` 等, 用于动画。

类型转换: int 与 float 可隐式互转; 带单位的值不能直接转数字 — 要除以或乘以 `1px`; 结构体按结构转换; 字符串转数字用 `str.to-float()` / `str.is-float()`。

结构体与枚举:

```slint
export component Example {
    property<{a: string, b: int}> s: { a: "x", b: 12 };
    property<[TileData]> tiles: [ { image: @image-url("icon.png") } ];
}
```

## 4. 元素 (视觉基元)

基础元素: `Rectangle` (background、border-width、border-radius、clip)、`Text` (text、font-size、color、horizontal-alignment)、`Image` (source、width、height、image-fit)、`Path`、`StyledText`。

**所有可见元素共有属性:**

- 几何: `x`、`y`、`width`、`height`、`z` (层叠顺序, 编译期常量)、`absolute-position` (out Point, 相对窗口坐标)。

- 布局约束: `min-width/height`、`max-width/height`、`preferred-width/height`、`horizontal-stretch`、`vertical-stretch` (in-out float, 0 表示不拉伸)。

- 显示: `opacity` (0 到 1 或百分比)、`visible` (false 时不可见且不响应输入)、`cache-rendering-hint` (缓存子树的渲染提示)。

- 无障碍: `accessible-role`、`accessible-label`、`accessible-description`、`accessible-checked`、`accessible-value` 等 (须先设 `accessible-role` 才能用其它 accessible 属性)。

交互: `TouchArea` — 标准输入处理元素:

```slint
ta := TouchArea {
    clicked => { root.hello(); }   // 还有: pointer-event、moved、has-hover、is-pressed、pressed/released
}
```

滚动: `Flickable`。文本输入: `TextInput` (底层) 或 `LineEdit` (部件)。键盘: `FocusScope` (key-pressed、key-released、focus 事件)。手势: `SwipeGestureHandler`、`ScaleRotateGestureHandler`。窗口/弹窗: `Window`、`Dialog`、`PopupWindow`、`ContextMenuArea`、`Tooltip`。

## 5. 布局

两种定位模型:

1. **显式定位**: 直接在子元素上设置 `x`、`y`、`width`、`height`。
2. **自动布局**: `HorizontalLayout`、`VerticalLayout`、`GridLayout`。

```slint
VerticalLayout {
    alignment: center;            // 主轴对齐, 见 5a 节完整取值
    spacing: 8px;                 // 子元素间距
    padding: 12px;                // 内边距
    Text { text: "Header"; }
    HorizontalLayout {
        Rectangle { background: blue; }        // 默认均分空间
        Rectangle { horizontal-stretch: 2; }    // 相对拉伸因子
    }
}
for item in model : Rectangle { }               // 布局内的重复元素
```

GridLayout: 用 `Row { ... }` 和 `Column { ... }` 组织, 或用 `GridLayout.Row` 配合 `colspan`/`rowspan`。
容器组件用 `@children` 嵌入使用者提供的子元素。

## 5a. 对齐速查 (水平居中 / 垂直居中 / 底边 / 左 / 右)

Slint 中对齐控件有三种方法: 布局属性、Text 元素属性、显式坐标表达式。

### 方法 A: 布局对齐属性 (子元素在布局内)

HorizontalLayout 和 VerticalLayout 各有两个对齐属性:

- `alignment` — 主轴方向对齐。HorizontalLayout 的主轴是水平方向, VerticalLayout 的主轴是垂直方向。可选值: `stretch` (默认)、`start`、`center`、`end`、`space-between`、`space-around`、`space-evenly`。

- `cross-axis-alignment` — 交叉轴方向对齐。HorizontalLayout 的交叉轴是垂直方向, VerticalLayout 的交叉轴是水平方向。可选值: `stretch` (默认, 子元素填满交叉轴)、`start` (顶边/左边)、`center` (居中)、`end` (底边/右边)。取非 stretch 值时子元素用自身 preferred 尺寸 (受 min/max 约束), 再定位到布局内容框的顶部、中间或底部。

水平居中 (在 VerticalLayout 中):

```slint
VerticalLayout {
    alignment: center;               // 整组子元素在垂直方向居中
    cross-axis-alignment: center;    // 每个子元素在水平方向居中
    Rectangle { width: 50px; height: 50px; }
}
```

垂直居中 (在 HorizontalLayout 中):

```slint
HorizontalLayout {
    alignment: center;               // 整组子元素在水平方向居中
    cross-axis-alignment: center;     // 每个子元素在垂直方向居中
    Rectangle { width: 50px; height: 50px; }
}
```

底边对齐 (在 VerticalLayout 中, 子元素压到底边):

```slint
VerticalLayout {
    alignment: end;                   // 剩余空间放在最前面, 子元素沉底
    cross-axis-alignment: center;     // 同时水平居中
    Rectangle { width: 50px; height: 50px; }
}
```

左对齐 (在 VerticalLayout 中, 交叉轴向左):

```slint
VerticalLayout {
    cross-axis-alignment: start;     // 子元素靠左
    // cross-axis-alignment: end;     // 子元素靠右
    // cross-axis-alignment: center;  // 子元素水平居中
    // cross-axis-alignment: stretch; // 子元素填满宽度 (默认)
    Rectangle { width: 50px; height: 50px; }
}
```

右对齐 (在 VerticalLayout 中, 交叉轴向右):

```slint
VerticalLayout {
    cross-axis-alignment: end;        // 子元素靠右
    Rectangle { width: 50px; height: 50px; }
}
```

顶边/底边对齐 (在 HorizontalLayout 中, 交叉轴):

```slint
HorizontalLayout {
    cross-axis-alignment: start;      // 子元素靠顶边
    // cross-axis-alignment: end;     // 子元素靠底边
    // cross-axis-alignment: center;  // 子元素垂直居中
    // cross-axis-alignment: stretch; // 子元素填满高度 (默认)
    Rectangle { width: 50px; height: 50px; }
}
```

左对齐/右对齐 (在 HorizontalLayout 中, 主轴):

```slint
HorizontalLayout {
    alignment: start;    // 子元素整组靠左, 剩余空间在末尾
    // alignment: end;   // 子元素整组靠右, 剩余空间在开头
    Rectangle { width: 50px; height: 50px; }
}
```

主轴 alignment 各值含义 (语义与 CSS flexbox 一致):

- `stretch` — 所有元素取最小尺寸, 剩余空间按各元素的 `*-stretch` 因子分配 (默认)

- `start` — 所有元素取 preferred 尺寸, 剩余空间全部放在最后一个元素之后

- `end` — 所有元素取 preferred 尺寸, 剩余空间全部放在第一个元素之前

- `center` — 所有元素取 preferred 尺寸, 剩余空间平分在首元素前和末元素后

- `space-between` — 剩余空间平均分配在相邻元素之间

- `space-around` — 类似 space-between, 但首尾各再加半个间距

- `space-evenly` — 剩余空间平均分配在首元素前、末元素后以及元素之间

### 方法 B: Text 元素对齐 (文字在自身边界内)

Text 元素在自己的矩形框内对齐其文字内容:

```slint
Text {
    text: "Hello";
    // 水平对齐, 可选值: start | end | left | center | right
    horizontal-alignment: center;
    // 垂直对齐, 可选值: top | center | bottom
    vertical-alignment: center;
}
```

说明: `start`/`end` 跟随文字书写方向 (从右到左语言会翻转); `left`/`right` 是绝对方向。Text 元素需要比文字本身大 (设置了 width/height 或放在布局中) 才能看到对齐效果。

### 方法 C: 显式坐标表达式 (无布局时, 任何元素可用)

子元素直接放在容器里、不使用布局时, 通过计算 x/y 相对父元素定位:

```slint
export component MainWindow inherits Window {
    width: 300px;
    height: 200px;

    // 水平居中
    Rectangle {
        width: 50px; height: 50px;
        x: (parent.width - self.width) / 2;
        y: 0px;
    }

    // 垂直居中
    Rectangle {
        width: 50px; height: 50px;
        x: 0px;
        y: (parent.height - self.height) / 2;
    }

    // 水平且垂直居中
    Rectangle {
        width: 50px; height: 50px;
        x: (parent.width - self.width) / 2;
        y: (parent.height - self.height) / 2;
    }

    // 底边对齐
    Rectangle {
        width: 50px; height: 50px;
        x: 0px;
        y: parent.height - self.height;
    }

    // 左对齐
    Rectangle {
        width: 50px; height: 50px;
        x: 0px;
    }

    // 右对齐
    Rectangle {
        width: 50px; height: 50px;
        x: parent.width - self.width;
    }

    // 右下角对齐
    Rectangle {
        width: 50px; height: 50px;
        x: parent.width - self.width;
        y: parent.height - self.height;
    }
}
```

坐标规则: x 为 0 是左边缘, y 为 0 是顶边缘; 右边缘是 `parent.width - self.width`, 底边缘是 `parent.height - self.height`; 居中就是 `(parent.size - self.size) / 2`。

### 选择建议

- 子元素在 HorizontalLayout/VerticalLayout 里: 用 `alignment` 加 `cross-axis-alignment`。

- Text 内部文字对齐: 用 Text 的 `horizontal-alignment` / `vertical-alignment`。

- 不在布局里的自由元素: 用显式 x/y 表达式。

- 单个居中的子元素: `x: (parent.width - self.width) / 2;` 或包一层布局并设两个方向 alignment 为 center。

## 6. 重复与模型

```slint
for tile[i] in memory_tiles : MemoryTile {
    x: mod(i, 4) * 74px;
    icon: tile.image;
}
```

`for name[index] in model:` — model 可以是数组属性, 也可以是宿主语言提供的模型对象。`if condition : Element { }` — 条件元素。

## 7. 状态与过渡

```slint
states [
    disabled when !root.is-enabled : {
        background: gray;
        text.color: white;
        out { animate * { duration: 800ms; } }   // 离开该状态时动画
    }
    down when pressed : {
        background: blue;
        in { animate background { duration: 300ms; } }  // 进入该状态时动画
    }
]
```

状态语法: `state-name when <条件> : { <属性赋值> }`。过渡: `in` (进入时)、`out` (离开时)、`in-out` (两者)。

## 8. 动画

```slint
Rectangle {
    background: ta.is-pressed ? blue : red;
    animate background { duration: 250ms; easing: ease-in-out; }
    ta := TouchArea {}
}
```

用 `animate <property> { duration; easing; delay; iteration-count; }` 让属性变化产生动画。

## 9. 函数与回调

函数 — 完全在 Slint 内定义的逻辑:

```slint
export component Example {
    public pure function double(x: int) -> int {
        return x * 2;
    }
}
```

- 默认可见性为 private; `public` 允许其他组件和宿主语言调用; `protected` 仅限子类。

- `pure` 表示无副作用 (优先用于绑定中)。

回调 — 与宿主语言的桥梁:

```slint
export component MainWindow inherits Window {
    callback check-if-pair-solved();        // 声明
    in property <bool> disable-tiles;       // 宿主代码可设置

    area := TouchArea {
        clicked => {                        // 处理内建回调
            root.check-if-pair-solved();    // 触发自定义回调
        }
    }
    callback clicked <=> area.clicked;      // 用 <=> 双向绑定做别名
}
```

回调可以带参数、返回值和命名参数: `callback hello(foo: int, bar: string);`。需要宿主语言处理事件时用回调而非函数。

`init => { ... }` — 组件实例化时执行一次, 用于只跑一次的初始化副作用; 普通绑定与函数须保持纯净 (无副作用)。

## 10. 全局单例

用于共享状态 / 主题:

```slint
export global Theme {
    in-out property <color> primary: #0066cc;
    public pure function spacing() -> length { 8px; }
}
// 任意位置使用: Theme.primary
```

标准部件库的全局单例: `Palette` (自动适配亮/暗主题的颜色)、`StyleMetrics`。

**主题/暗色模式:** 不要硬编码颜色, 用 `Palette` 的语义色 (如 `Palette.background`、`Palette.foreground`、`Palette.text`)。检测系统深浅色用 `Window` 的 `is-dark-color-scheme` 属性。自定义主题可建 `export global Theme { in-out property <color> ... }` 并在各处引用; 也可用 `@linear-gradient` / `@radial-gradient` 搭配 `Palette` 做自适应配色。

## 11. std-widgets 部件库

导入: `import { ... } from "std-widgets.slint";`

- **基础部件:** Button、StandardButton、CheckBox、ComboBox、Slider、SpinBox、Switch、RadioGroup、ProgressIndicator、Spinner

- **视图部件:** LineEdit、ListView、ScrollView、StandardListView、StandardTableView、TabWidget、TextEdit

- **布局部件:** HorizontalBox、VerticalBox、GridBox、GroupBox

- **其他:** AboutSlint、DatePickerPopup、TimePickerPopup

部件样式: `fluent`、`material`、`cupertino`、`native`。默认 `native` (仅安装了 Qt 时可用), 否则回退 `fluent`。选择方式: `SLINT_STYLE` 环境变量, 或编译配置 (Rust: `compile_with_config` / `ComponentCompiler::set_style`; C++: `SLINT_STYLE` CMake 变量; slint-viewer: `--style material`)。

## 12. 宿主语言集成

**Rust** (依赖 `slint` crate, 编译后的 `.slint` 用 `slint::include_modules!()`):

```rust
slint::include_modules!();
fn main() -> Result<(), slint::PlatformError> {
    let main_window = MainWindow::new()?;
    main_window.run()
}
```

- 内联 UI 用 `slint! { ... }` 宏; Cargo.toml 的 `[build-dependencies]` 加 `slint-build`, 在 `build.rs` 里调用 `slint_build::compile("ui/app-window.slint")`。

- 属性: `main_window.set_disable_tiles(true)` / `.get_x()`; 回调: `main_window.on_check_if_pair_solved(|| { ... })`。

**C++** (CMake ≥ 3.21, C++20):

```cmake
slint_target_sources(my_application ui/app-window.slint)
```

```cpp
#include "app-window.h"  // 由 ui/app-window.slint 生成
auto main_window = MainWindow::create();
main_window->run();
```

**JavaScript/Node.js:**

```js
import * as slint from "slint-ui";
const ui = slint.loadFile(new URL("./ui/app-window.slint", import.meta.url));
const mainWindow = new ui.MainWindow();
await mainWindow.run();
```

**Python:**

```python
import slint
class MainWindow(slint.loader.ui.app_window.MainWindow):
    pass
main_window = MainWindow()
main_window.show()
main_window.run()
```

Python 通过 `slint.loader` / `loadFile` 加载, 名称从 kebab-case 映射为 snake\_case。

## 13. 工具链

- **VS Code**: 安装 Slint 扩展; 实时预览; 命令面板提供项目模板 (`Slint: Create New Project from Template`)。

- **Slint LSP**: 支持各编辑器 (Kate、Vim、Helix、Sublime、JetBrains、Zed、Qt Creator)。

- **Slint Viewer**: `slint-viewer` 无需宿主代码即可预览 `.slint` 文件。

- **SlintPad** (<https://slintpad.com/>): 浏览器在线编辑器。

- 项目模板: <https://github.com/slint-ui/slint-rust-template> (另有 -cpp-、-nodejs-、-python- 版本)。

## 14. 最佳实践

1. **UI 用 Slint 声明, 逻辑用宿主语言实现。** 输入用属性, 输出用回调。
2. 能用声明式绑定就不用 changed 回调; 滥用 `changed` 回调可能造成无限循环 (Slint 会在几次迭代后中断 — 属未定义行为)。
3. 可缩放的 UI 优先用布局而非绝对定位。
4. 主题色用 `Palette` 全局单例, 不要硬编码。
5. 属性/状态/回调/元素名用 kebab-case (`my-property`), 宿主语言 API 会转换为 snake\_case 或 camelCase。
6. 动画要克制; 简单的两态动画直接把 `animate` 挂在属性上而不是状态里。
7. 带业务逻辑的重复列表, 暴露一个 `in-out property <[StructType]>`, 让宿主语言修改模型。

## 15. 官方文档索引 (深入查阅)

- 语言指南 → .slint 文件: <https://docs.slint.dev/latest/docs/slint/guide/language/coding/file>

- 语言指南 → 响应式 (Reactivity): <https://docs.slint.dev/latest/docs/slint/guide/language/concepts/reactivity/>

- 参考 → 类型 (Types): <https://docs.slint.dev/latest/docs/slint/reference/primitive-types>

- 参考 → 元素 (如 Text): <https://docs.slint.dev/latest/docs/slint/reference/elements/text/>

- 参考 → std-widgets (如 ListView): <https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/listview/>

- 教程 (记忆游戏): <https://docs.slint.dev/latest/docs/slint/tutorial/quickstart/>

- Rust API: <https://docs.slint.dev/latest/docs/rust/slint/>

- C++ API: <https://docs.slint.dev/latest/docs/cpp/>

- Node.js API: <https://docs.slint.dev/latest/docs/node/>

- Python API: <https://docs.slint.dev/latest/docs/python/>

## 快速参考: 最小应用

```slint
// ui/app-window.slint
import { Button, VerticalBox } from "std-widgets.slint";

export component MainWindow inherits Window {
    preferred-width: 200px;
    preferred-height: 120px;
    title: "Demo";
    callback quit();
    VerticalBox {
        Text { text: "Hello world"; }
        Button { text: "Quit"; clicked => { root.quit(); } }
    }
}
```

```rust
// src/main.rs
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let main_window = MainWindow::new()?;
    main_window.on_quit(|| std::process::exit(0));
    main_window.run()
}
```

