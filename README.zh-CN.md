<p align="center">
  <img src="docs/banner.svg" alt="git-include" width="720">
</p>

**[English](README.md)** | **[Deutsch](README.de.md)** | **中文**

`git-include` 是 [git-subrepo](https://github.com/ingydotnet/git-subrepo) 的现代化单文件替代品，使用 Rust 编写。它会把一个上游仓库以子目录的形式引入到你的仓库中，再加上一个小小的标记文件。这就是它的全部模型：

- **协作者什么都不用装。** 他们只需 `git clone` 就能拿到可运行的代码，不需要 `--recursive`、不需要 `submodule update`、也不需要安装 git-include。只有负责与上游同步的人才需要这个工具。
- **双向同步。** `git include pull` 会把上游的新工作合并进你的目录树；`git include push` 会根据你的提交重建上游历史——每一个改动过该目录的宿主提交都会变成一个独立的上游提交，保留原始提交信息和作者（即使是在某次 pull 之前做的提交也一样），标记文件永远不会泄漏到上游。
- **与 git-subrepo 兼容。** 标记文件使用同样的 `.gitrepo` 格式。已经在用 git-subrepo 的仓库可以零迁移直接采用。
- **内置导出功能。** `git include init` 可以把任意一个普通目录变成一个新的被引入仓库，并从你的提交历史中提取出它的完整历史——随时可以推送到它自己的（哪怕是空的）仓库。
- 开箱即用的**分支切换**、快速的**与上游的状态/差异对比**、**嵌套引入**以及 **Tab 自动补全**。

```console
$ git include add https://github.com/example/widgets vendor/widgets
$ git include status
$ git include pull vendor/widgets      # 获取上游的新工作
$ git include push vendor/widgets      # 把自己的改动贡献回去
```

---

## 目录

- [为什么不用 submodule / subtree / subrepo?](#为什么不用-submodule--subtree--subrepo)
- [安装](#安装)
- [Tab 自动补全](#tab-自动补全)
- [快速开始](#快速开始)
- [命令参考](#命令参考)
- [从 submodule 迁移](#从-submodule-迁移)
- [固定到标签和提交](#固定到标签和提交)
- [自定义提交信息](#自定义提交信息)
- [`.gitrepo` 标记文件](#gitrepo-标记文件)
- [Git LFS](#git-lfs)
- [将目录导出为独立仓库](#将目录导出为独立仓库)
- [嵌套引入](#嵌套引入)
- [处理合并冲突](#处理合并冲突)
- [工作原理](#工作原理)
- [常见问题](#常见问题)
- [开发](#开发)

---

## 为什么不用 submodule / subtree / subrepo?

submodule 会让每个协作者都付出代价（额外的工具、`--recursive`、detached-HEAD 的意外状况）；subtree 会把合并噪音混进历史记录，并以一种难以查看的方式隐藏自己的状态；这两者都会让「与上游的差异对比」「切换跟踪的分支」「还有什么没推送」这类日常操作变得别扭甚至不可能。

这里的基本理念和 git-subrepo 一样：**被引入的代码只是你仓库里的普通文件**，一个标记文件记录了它们从哪里来、对应上游的哪个提交。其余的一切——合并、推送、差异对比——都是从这个标记文件推导出来的。

与 git-subrepo 相比，git-include 是一个用 Rust 编写、编译出来的二进制程序（基于 `git2` crate 之上的 libgit2），而不是用 bash 写的脚本——Rust 是一门强类型、具有编译期保证的语言。它也从不在你的仓库里创建临时分支、工作树或克隆:你的分支和工作目录除了正在操作的那一个子目录之外都不会被触碰。它的命令行更加直观；支持固定到某个具体的标签或提交（而不只是分支），也支持 Git LFS 以及直接迁移现有的 submodule。

## 安装

**Linux / macOS —— 一行命令：**

```console
$ curl -fsSL https://raw.githubusercontent.com/flova/git-include/main/install.sh | bash
```

这个脚本会检测你的平台，下载最新的发布版二进制文件，用发布版的 `SHA256SUMS` 清单校验它，然后安装到 `~/.local/bin`（如果是 root 用户则是 `/usr/local/bin`）。对于 Linux，会发布两种版本，脚本会自动选择（可用 `GIT_INCLUDE_FLAVOR=dynamic|portable` 覆盖）：

- `*-linux-gnu` —— 动态链接到发行版自带的 OpenSSL 和 zlib，不打包任何东西。系统兼容时优先使用这个版本。
- `*-linux-gnu-portable` —— **内置编译**了 OpenSSL 和 zlib，只需要 glibc ≥ 2.28（2018 年发布），因此可以在老旧发行版和没有 libssl 的精简容器镜像上运行。（像 Alpine 这样基于 musl 的发行版需要从源码编译——见下文。）

macOS 上的二进制文件使用系统自带的 Security 框架来处理 TLS；只有 SSH 支持部分内置编译了 OpenSSL（因为 macOS 没有自带可供链接的 OpenSSL）。你也可以直接从[发布页面](https://github.com/flova/git-include/releases)下载对应平台的二进制文件。用 `GIT_INCLUDE_VERSION=v0.1.0` 固定版本，用 `GIT_INCLUDE_BIN_DIR` 修改安装目录。随时可以更新——这个二进制文件会自我更新：

```console
$ git include self-update            # 或者 --version vX.Y.Z，或者用 -n 预览
```

（自我更新下载的文件会先用发布版的 `SHA256SUMS` 清单校验，然后才会替换正在运行的二进制文件。）

（自我更新功能只编译进了 git-include 自己分发的二进制文件——也就是通过 curl 安装的版本和 Windows 的 MSI 安装包。像 conda 这样的包管理器构建会通过一个 Cargo feature flag 禁用它。）

**Windows：** 从[最新发布版](https://github.com/flova/git-include/releases/latest)下载 MSI 安装包（x64）——它会安装 `git-include.exe` 并把它加入 `PATH`。在 ARM64 版 Windows 上，改为从发布版的附件里下载 `git-include-aarch64-pc-windows-msvc.exe`，然后自己把它加入 `PATH`。（`self-update` 在 Windows 上同样可用，两种架构都支持。）

**Conda：** 每个发布版都会提供 linux-64、linux-aarch64、osx-arm64 和 win-64 的 `.conda` 包（见发布版的附件；配方文件在 `conda/recipe.yaml` 里）。目前没有预编译的 Intel Mac 包或二进制文件——Intel Mac 用户需要从源码编译（见下文）。Conda 构建的版本没有自我更新机制——在这种情况下更新是 conda 的职责（`conda update git-include`），`git include self-update` 也会相应地提示你，而不是去和包管理器打架。

**从源码编译**（需要一个较新的 stable Rust；libgit2 是内置编译的，所以除了 Linux 上的 OpenSSL 之外没有其他系统依赖）：

```console
$ cargo install --git https://github.com/flova/git-include   # 直接从 GitHub 安装
$ cargo install --path .                                     # 从本地检出的代码安装
```

这个二进制文件名为 `git-include`，所以 git 会自动把它识别为一个子命令：`git include <command>`。用下面的命令验证一下：

```console
$ git include --version
```

## Tab 自动补全

为你的 shell 生成一个补全脚本，并从 shell 配置文件中引入它：

```console
# bash —— 同时补全 `git-include <TAB>` 和 `git include <TAB>`，
# 包括对已引入目录和分支名称的实时补全
$ git include completions bash > ~/.local/share/bash-completion/completions/git-include

# zsh —— 放到你自己的 $fpath 里；zsh 的 git 补全会自动转发过来
$ git include completions zsh > ~/.zfunc/_git-include

# fish
$ git include completions fish > ~/.config/fish/completions/git-include.fish
```

同时也支持 Elvish 和 PowerShell（见 `git include completions --help`）。

## 快速开始

### 引入一个仓库

```console
$ git include add https://github.com/example/widgets vendor/widgets
No branch given; using upstream default branch 'main'.
Fetching https://github.com/example/widgets (main) ...
Added 'vendor/widgets' from https://github.com/example/widgets (branch main, commit 1a2b3c4).
```

这会在你的仓库里创建**一个提交**，其中包含 `vendor/widgets/` 下完整的上游目录树，以及 `vendor/widgets/.gitrepo`。从此以后这个目录就是完全普通的：编辑它、提交它、还原它、用 bisect 定位问题——它就是一堆普通文件。

### 查看当前状态

```console
$ git include status --fetch
vendor/widgets
  remote:   https://github.com/example/widgets
  branch:   main (synced at 1a2b3c4)
  upstream: 2 new commit(s) available -> `git include pull vendor/widgets`
  local:    1 commit(s) to push -> `git include push vendor/widgets`

$ git include diff vendor/widgets              # 自上次同步以来自己的改动
$ git include diff vendor/widgets --upstream --fetch   # 与最新上游状态的对比
```

当输出到终端时，`diff` 的输出会像 `git diff` 一样着色（可以用标准的 `NO_COLOR` 环境变量禁用）。

不加 `--fetch` 时，`status` 使用的是最近一次 fetch 时看到的上游状态，因此它是即时的，并且可以离线使用。

### 拉取上游的改动

```console
$ git include pull vendor/widgets
```

如果你对该目录有本地改动，它们会和上游的改动做三方合并，就像一次 `git merge` 一样——包括内容级别的合并，以及当双方改动了同样的行时产生的冲突标记。结果是你仓库里的一个单独提交。`git include pull --all` 会同步所有已引入的目录；如果只有一个引入，直接用 `git include pull` 就够了。

如果目录的本地状态已经没法要了，`git include pull --force` 会丢弃它——不管有没有提交——并直接采用上游的内容。被强制丢弃的改动也会被排除在未来的推送之外。

### 把自己的改动推送到上游

```console
$ git include push vendor/widgets
Pushed 2 commit(s) from 'vendor/widgets' to https://github.com/example/widgets (main); upstream is now 9f8e7d6.
```

`push` 会把上游历史重建为你**宿主提交的一比一镜像**：自从上次把改动同步到上游以来，每一个改动过该目录的提交都会变成它自己的上游提交——保留原始提交信息、原始作者，只包含该目录下的文件。分支和合并会被完全按照它们在宿主仓库里发生的样子镜像过去（一个解决了分支冲突的宿主合并提交，到了上游会是同样的合并提交，带着同样的冲突解决方案）；从未改动过该目录的提交则会被排除在外。这在**跨越多次 pull** 时依然有效：在某次 pull 之前做的提交仍然是独立的提交，基于它们实际编写时所依据的上游状态，而那次 pull 本身则会变成与上游自身历史的一次普通合并。提交的哈希值必然会和宿主提交不同，但内容和拓扑结构会被精确保留。`.gitrepo` 标记会被自动剥离，永远不会出现在上游。

用 `git include push -n <dir>` 预览；如果你更想把所有改动发布成一个单独的提交，用 `--squash`。

如果上游在此期间发生了变化，`push` 会拒绝执行，并要求你先执行 `git include pull`，这样上游就永远不会得到一个意外的合并结果。

推送也可以指向**不同的分支和/或不同的远程仓库**——比如一个功能分支，或者一个 fork：

```console
$ git include push vendor/widgets --branch feature/my-fix
$ git include push vendor/widgets --remote git@github.com:me/widgets-fork -b pr/fix --keep
```

默认情况下，该引入会被**重新指向**推送的目标（标记文件会记录新的远程仓库/分支，未来的 pull 也会跟随它）。如果是临时 fork 的工作流程，可以加上 `--keep`：推送照常进行，但标记文件仍然跟踪原来的状态——一旦这个提议在上游被合并，一次普通的 `pull` 就会把它同步过来。这两种方式对于固定到某个标签或提交的引入同样有效（用 `--branch` 指定目标）。已经存在的目标分支只有在处于记录的基准状态时才会被接受，因此不相关的工作永远不会被覆盖。

`pull` 和 `switch` 同样接受 `--remote <url>`——pull 总是会把标记重新指向它实际拉取的那个远程仓库。这也让 `pull --remote` 成为跟随一个搬了家的上游的方式：即使内容没有变化，从新地址拉取也会重新指向这个引入。

### 切换跟踪的分支

```console
$ git include branches vendor/widgets
* main (1a2b3c4)
  next (5d6e7f8)

$ git include switch vendor/widgets next
Switched 'vendor/widgets' to branch next (commit 5d6e7f8).
```

切换时本地改动会被带过去（合并）；如果目录是干净的，就直接换成新分支的内容。切换回去用的是同一个命令。`switch` 也接受标签或提交 ID——见[固定到标签和提交](#固定到标签和提交)。

## 命令参考

| 命令 | 说明 |
| --- | --- |
| `git include add <remote> <dir> [-b <branch> \| -t <tag> \| --commit <sha>]` | 把一个上游仓库引入到 `<dir>`，跟踪某个分支（默认：远程仓库的默认分支），或者固定到某个标签/提交。 |
| `git include pull [<dir>] [--all] [--force] [-r <url>]` | 把新的上游提交合并进 `<dir>`（或所有引入）；`--force` 丢弃本地改动，`-r` 从另一个远程仓库拉取（并重新指向它）。 |
| `git include push <dir> [-n] [-b <branch>] [-r <url>] [--keep] [--squash]` | 把涉及 `<dir>` 的本地提交重放到上游分支并推送；`-b`/`-r` 推送到别处（并重新指向）,`--keep` 保留当前的跟踪状态。 |
| `git include status [<dir>] [-f/--fetch]` | 显示同步状态：上游可用的提交、待推送的提交、未提交的改动。 |
| `git include diff <dir> [--upstream] [--stat] [-f/--fetch]` | 把 `<dir>` 和上次同步的提交对比，或者和最新的上游状态对比。 |
| `git include switch <dir> <branch\|tag\|commit>` `[-r <url>]` | 跟踪一个不同的分支，或者固定到某个标签/提交，本地改动会被带过去；`-r` 同时切换远程仓库。 |
| `git include branches <dir>` | 列出上游的分支和标签，标出当前跟踪的状态。 |
| `git include migrate [<path>...]` | 把 git submodule 转换为引入——可以是全部，也可以只转换指定的路径。 |
| `git include list` | 列出所有引入，嵌套的会缩进显示。 |
| `git include remove <dir>` | 从工作目录中删除一个引入（历史记录和上游都不受影响）。 |
| `git include completions <shell>` | 输出一个 Tab 自动补全脚本。 |
| `git include self-update [--version <tag>]` | 把 git-include 二进制文件更新到最新（或指定的）发布版本。 |

所有 `<dir>` 参数都是相对于当前目录的，因此这些命令可以在仓库内的任何地方执行。`--no-lfs` 被 `add`、`pull`、`push` 和 `switch` 接受，用于跳过 LFS 传输；`-m/--message` 被所有会创建同步提交的命令接受（见[自定义提交信息](#自定义提交信息)）。

## 从 submodule 迁移

一条命令就能把一个基于 submodule 的仓库转换成基于引入的仓库：

```console
$ git include migrate                # 转换每一个 submodule
$ git include migrate vendor/lib     # 或者只转换这一个
Migrating submodule 'vendor/lib' (recorded commit 1a2b3c4) ...
Migrated 'vendor/lib' -> include of https://github.com/example/lib pinned to commit 1a2b3c4.
```

每个 submodule 都会变成一个**固定在该 submodule 所记录的那个确切提交上**的引入，因此这次迁移不会改变你目录树的任何内容——每个 submodule 对应一个提交，把 gitlink 转换成带有 `.gitrepo` 标记的普通文件。`.gitmodules` 里的条目会被移除（如果文件变空了就会被删除），submodule 遗留下来的 `.git/modules` 克隆以及 `submodule.*` 配置也会被清理掉。之后，可以用 `git include switch <dir> <branch>` 把任何一个引入从固定状态切换到一个活跃分支。

## 固定到标签和提交

和 git-subrepo 不同，一个引入不一定要跟踪某个分支——它也可以被固定到一个确切的上游状态：

```console
$ git include add https://github.com/example/widgets vendor/widgets --tag v2.1.0
$ git include add https://github.com/example/parser  vendor/parser  --commit 9f8e7d6c...
$ git include switch vendor/widgets v2.2.0     # 在不同发布版本之间切换
$ git include switch vendor/widgets main       # 切回跟踪某个分支
```

`switch` 会自动解析它的参数（先当作分支，然后是标签，最后是提交 ID），所以无论是在发布版本之间切换，还是切回分支跟踪，都只需要一条命令。一个被固定的引入是完全可复现的：`pull` 会报告这个固定状态而不是移动它，`status`/`diff` 会和固定的状态做对比，`push` 会拒绝执行并提示使用 `switch`（因为没有分支可以推送）。切换时本地改动会被带过去——或者用 `switch --force` 丢弃它们。

## 自定义提交信息

git-include 创建的同步提交（add、pull、switch、push 的记账提交、init、remove）的提交信息，可以通过 Jinja 模板（借助 [minijinja](https://crates.io/crates/minijinja)）自定义——变量、过滤器和条件语句都可以使用：

```console
# 针对整个仓库（或加 --global），应用于所有同步提交：
$ git config include.commitTemplate 'chore(vendor): {{ action }} {{ subdir }} @ {{ short_commit }}'

# 或者针对单次调用：
$ git include pull vendor/widgets -m 'vendor: update widgets to {{ short_commit }}'

# 完整的 Jinja 表达式也可以使用：
$ git include pull vendor/widgets \
    -m '{% if action == "pull" %}⬆{% endif %} {{ subdir | upper }} @ {{ short_commit }}'
```

| 变量 | 值 |
| --- | --- |
| `{{ action }}` | 执行的命令，包括重要的 flag（例如 `pull --force`） |
| `{{ subdir }}` | 被引入的目录 |
| `{{ remote }}` | 上游 URL |
| `{{ ref }}`（别名 `{{ branch }}`） | 跟踪的分支/标签/提交 |
| `{{ ref_kind }}` | `branch`、`tag` 或 `commit` |
| `{{ commit }}` / `{{ short_commit }}` | 上游提交（完整版 / 7 位缩写） |
| `{{ version }}` | git-include 的版本号 |

字面上的 `\n` 序列会变成换行符，因此多行的提交信息也能塞进单行的 config 值里。一个有问题的模板（语法错误或未知变量）只会打印一条警告并回退到默认信息——一次已经完成的同步不会因为一个笔误而被中止。如果没有设置模板，git-include 会写入它结构化的默认信息（`git include <action> <dir>` 加上一个元数据区块）。

## `.gitrepo` 标记文件

每个被引入的目录都包含一个 git-subrepo 格式的 `.gitrepo` 文件：

```ini
; DO NOT EDIT (unless you know what you are doing)
;
[subrepo]
	remote = https://github.com/example/widgets
	branch = main
	commit = 1a2b3c4d...   ; upstream commit the directory was last synced to
	parent = 9z8y7x6w...   ; last host commit whose changes are already upstream
	method = merge
	cmdver = 0.1.0
```

由于格式、字段名和语义都和 git-subrepo 一致，在一个已经使用 git-subrepo 的项目里引入 git-include 完全不需要迁移：它可以直接操作用 `git subrepo clone` 引入的目录。反过来的方向对于跟踪分支的引入同样适用，但要注意：git-subrepo 没有固定到标签或提交的概念——一个使用了这些功能的引入在 git-subrepo 里没有对应的东西。

## Git LFS

如果上游仓库使用了 Git LFS，git-include 会（通过其 `.gitattributes` 里的 `filter=lfs`）注意到这一点，并自动处理它：

- **add / pull / switch** 会从*上游*的 LFS 存储中获取 LFS 对象，并在你的工作目录里落地真实内容，
- **push** 会在推送 git 对象*之前*先上传你的提交所引用的 LFS 对象，这样上游永远不会看到悬空的指针文件，
- 如果没有安装 `git-lfs`，这些操作依然会成功——只不过你会得到指针文件，外加一条清晰的警告，告诉你之后要运行哪些命令，
- `--no-lfs` 会跳过这一切。

## 将目录导出为独立仓库

这是 `add` 的反向操作：一个在你仓库内部逐渐成长起来的目录，可以连同它的历史一起，升级成一个独立的仓库。

```console
$ git include init mylib --remote git@github.com:me/mylib.git
Extracting the history of 'mylib' ...
Turned 'mylib' into an included repository: extracted 17 commit(s) of history (head 3fc9a21).
Publish it with: git include push mylib

$ git include push mylib
Published 'mylib' to git@github.com:me/mylib.git as new branch 'main'.
```

`init`（别名 `export`）会遍历你的整个历史，每一个改动过该目录的提交都会变成一个全新独立历史中的提交——保留原始作者和信息，内容则过滤到只剩该目录的部分（一个同时改动了 `mylib/` 和其他文件的提交，只会贡献它 `mylib/` 部分的改动）。然后 `push` 会发布这段历史，如果需要的话会在一个空的远程仓库上创建分支。从这一刻起，这个目录就是一个普通的引入了：其他人可以用 `git include add` 引入它，`pull`/`push`/`status` 也都能照常使用。

## 嵌套引入

被引入的仓库自己也可以包含引入。因为一切都只是普通文件，内层的 `.gitrepo` 标记会自动跟着一起走：

```console
$ git include add https://github.com/example/app libs/app
$ git include list
libs/app  <-  https://github.com/example/app (main @ 4ee9c11)
  libs/app/vendor/parser  <-  https://github.com/example/parser (main @ 77af0d3)
```

你可以在任何一层进行操作：`git include pull libs/app` 会同步外层仓库（连带它已经提交的那个版本的 `vendor/parser`），而 `git include pull libs/app/vendor/parser` 则会直接从*它自己*的上游同步内层仓库。推送一个引入时，只有它自己的标记会被剥离——嵌套的标记属于内容，会原样一起被推送。

## 处理合并冲突

当你和上游都改动了同样的行时，`pull` 会停下来，把带有标准冲突标记的冲突文件留在你的工作目录里：

```console
$ git include pull vendor/widgets
CONFLICT: could not automatically merge upstream changes into 'vendor/widgets'.
Files with conflict markers:
  vendor/widgets/src/lib.rs

Resolve the conflicts, then finish with:
  git add vendor/widgets
  git commit
```

这里没有需要维护的特殊「continue」状态：解决冲突标记、`git add`、`git commit`——就结束了。（`.gitrepo` 的更新已经提前为你暂存好了。）如果你想直接放弃，`git reset --hard` 会恢复到 pull 之前的状态。

## 工作原理

每一个操作都是标记文件里那四个值（`remote`、`branch`、`commit`、`parent`）加上宿主仓库当前状态、以及上游远程仓库状态的一个纯函数——`.git/config` 里没有任何状态、没有注册的远程仓库、没有临时分支。这一切都通过 libgit2（也就是 `git2` crate）在进程内完成：

- `add` 获取上游分支，然后通过重写根目录树，把它的目录树嫁接到目标前缀之下——一个提交，与上游没有共享历史。
- `pull` 取三棵目录树——上次同步的上游提交的目录树（基准）、你当前的目录树（我方）、以及新的上游 HEAD 的目录树（对方）——交给 libgit2 的三方合并（包含重命名检测）。一次干净的合并会变成宿主仓库里的一个提交；冲突则会以标准的冲突标记落地到你的工作目录里。
- `push` 首先确认上游分支是否仍然停留在记录的基准状态上（这样结果在上游就是一次纯粹的快进,而不是一次意外的合并），然后把每一个改动过该目录的宿主提交映射为一个上游提交——子目录树原样采用、剥离标记，保留原始信息和作者，宿主的父提交也被翻译成它们各自的上游镜像，因此分支和合并结构能够原封不动地保留下来。纯粹的标记记账提交会被自动跳过，同步提交则会被映射为它们所采用的那个上游提交（一次合并了本地工作的 pull，会变成与上游的一次真正的合并）。只有引入自己的标记会被剥离；嵌套的 `.gitrepo` 文件属于内容，会原样一起传递到上游。
- 获取到的上游状态会被固定在 `refs/include/<dir>` 下，这样 `status` 和 `diff` 就能离线工作，获取到的对象也能在 `git gc` 中存活下来。

有一种细微的情况被专门处理了：宿主仓库的一次全新克隆拥有被引入的*目录树和 blob*（它们可以从宿主提交中访问到），但没有上游的*提交*对象。因此同步命令会按需从上游远程仓库重新获取，并且能检测到上游历史被重写（force-push）的情况，给出清晰的恢复方式，而不是生成一个错误的合并结果。

没有临时分支、没有 `.git/modules`、没有 stash、除了被引入的目录之外不会碰你的工作目录——而且和 git-subrepo 不同，不依赖 `git subtree` 那种压缩合并机制。

## 常见问题

**我的协作者需要 git-include 吗？**
不需要。被引入的目录就是普通文件。只有运行 `pull`/`push`/`switch` 的人才需要这个工具。

**`add` 会让我的仓库变得臃肿吗？**
进入你分支的是上游的*目录树*（一个快照），而不是它的历史记录。获取到的上游历史会留在本地对象存储里用于合并，但永远不会被推送到你自己的宿主远程仓库。

**我可以直接编辑被引入的文件吗？**
可以——这正是它的意义所在。像平常一样提交就行；`git include status` 会显示哪些改动还没有推送到上游。

**如果上游 force-push 了怎么办？**
`pull` 和 `push` 会检测到记录的提交在上游已经不存在了，并告诉你如何恢复。

**我需要哪个版本的 git？**
任何版本都行——git-include 内置了 libgit2，自己直接和远程仓库通信，所以它的运作不依赖于机器上安装的 git 版本。唯一可选的外部依赖是用于 LFS 内容的 `git-lfs`，你的凭证会以标准方式被读取（ssh-agent 和 git 的 credential helper）。

## 开发

开发和发布环境用 [pixi](https://pixi.sh) 固定——一条命令就能得到项目构建和测试所用的精确 Rust 工具链、git-lfs、C 编译器和 rattler-build（版本都锁定在 `pixi.lock` 里）：

```console
$ pixi run test               # 完整测试套件，包含 LFS 往返测试
$ pixi run lint               # rustfmt + clippy，和 CI 里跑的完全一样
$ pixi run build              # 为你的平台构建发布版二进制文件
$ pixi run -e build conda-build   # 构建 conda 包
```

测试套件相当详尽：它端到端地针对真实的 git 仓库，测试了双向同步、嵌套引入、Git LFS、submodule 迁移，以及诸如并发分支冲突之类的边界情况，并且每次改动都会在 CI 中运行。

这里没有独立的工具链设置——开发、CI 和发布都通过 pixi 完成。发布版本由 CI 从 `v*` 标签构建，完全使用 pixi 固定的工具链（`dist` 环境——没有 rustup，没有系统包）；发布工作流也可以手动触发一次演练，生成所有构建产物而不发布任何东西。

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
