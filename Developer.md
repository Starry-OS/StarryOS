# Developer Guide

## Introduction
Welcome to the Developer Guide! This document is designed to help developers 
understand the architecture, tools, and best practices for contributing to
our project.

## Mutil repo
Starry now uses a multi-repo architecture to separate different components
of the project. This allows for better modularity, easier maintenance, and
independent versioning of each component.

But it also brings some challenges, such as managing dependencies and ensuring
compatibility between different repositories. To address these challenges,
we have established some guidelines and tools to help developers navigate


### Git submodule
We have already added the submodule that we need to notice in the main repo.
To clone the main repo with all its submodules, use the following command:
```bash
git submodule update --init --recursive
```
you will find the submodule in the `local_crates` folder.

### Change Code in submodule
When you need to change the code in the submodule, you can follow these steps:

1. Navigate to the submodule directory:
   ```bash
   cd local_crates/<submodule_name>
   ```
2. Create a new branch for your changes:
   ```bash
    git checkout -b <your_branch_name>
    ```
3. Make your changes and commit them:
    ```bash
    git add .
    git commit -m "Your commit message"
    ```

4. Push your changes to the remote repository:
    ```bash
    git push origin <your_branch_name>
    ```

5. Change `Cargo.toml` in the main repo to point to the git repository:
   ```toml
   [patch.crates-io]
   <submodule_name> = { git = "<repository_url>", branch = "<your_branch_name>" }
   ```

6. Test
   ```bash
   cargo update && make run
   ```

#### Merge
After test,you can on submodule repo to create PR







