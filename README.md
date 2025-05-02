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

As you can see, it does work a bit. But it's not like i expected. I was like, okay easy tool, let's just "vibe code" it and it should be done. Well, like every vibe coder product this is so fucking trash that i want to kill myself. And now the code is a ugly mess with almost 1K lines (at least it have unit tests). How do improve this? The json converting over and over again seems so stupid, but i do not know what other way do this? Send the raw text to the LLM everytime? 
