# Writing-with-AI, AI grammar helper

## Usage

```bash
# Install Rust.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Install Ollama.
curl -fsSL https://ollama.com/install.sh | sh
rustup update
git clone https://github.com/akamee666/writinHelper.git
cd writinHelper
cargo run
```

![image](https://github.com/user-attachments/assets/5170cfde-0f46-4e0e-9f41-84dd23d6c757)


As you can see, it does work a bit, but the result is definetly not like i expected. I was like, okay i don't want to waste to much time into this, let's just "vibe code" it and it should be done in one day. Well, like every "vibe coder" product, this is so fucking trash that i want to kill myself. And now the code is a ugly mess with almost 1K lines (at least it has unit tests).

How do improve this? Converting json over and over again seems just a overkill, just by removing that and adjusting the prompt a bit should be enough to get a more useful hints.


## Todo ~

- [ ] Using sentencex-js to split the sentences and send them divided to the LLM. But how do i know which sentences were corrected and which were not? What about sentences that are not changing anymore? Should i keep displaying hints to them?
