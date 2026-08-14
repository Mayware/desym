# Desym

## Installation
| Provider | Package |
|----------|---------|
| AUR | [desym-git](https://aur.archlinux.org/packages/desym-git) |

## Usage
Depac takes a single argument, the path of the file containing the json configuration, described below. There is no other way to interact with the program.

```jsonc
{
    // Symlinks to create, the key is the where the symlink file goes, the source is the symlink source
    "symlinks": {
        "/home/pika/.config/nvim/init.lua": {
            "source": "/home/pika/PubDoots/modules/neovim/init.lua"
        },

        // Directories also work fine
        "/home/pika/.config/kitty": {
            "source": "/home/pika/PubDoots/modules/kitty/source"
        }
    },

    // Files to create, however, source now refers directly to the content of the file, not the path
    "files": {
        "/home/pika/.zshrc": {
            "source": "export EDITOR=nvim\nalias vi=\"nvim\"\n",

            // UID and GID to assign to the file
            "uid": 1000,
            "gid": 1000,

            // The mode to assign the file, in decimal, ensure it matches the regular octal
            // 420 decimal = 0644 octal
            "mode": 420
        },

        // Desym runs as whatever user you run it as, then tries to chown to the uid/gid you set. If you only ever manage
        // files your user has access to, you can just run it as your user, otherwise, run it as root
        "/etc/example.conf": {
            "source": "example=true\n",
            "uid": 0,
            "gid": 0,
            "mode": 420
        }
    }
}
```
Note, only json, not jsonc is supported. The comments above are purely illustrative. 

Desym is intended to be used as part of a larger system, ideally with a config that generates the json for you.\
To see desym in action as a component in an advanced configuration, see [PubDoots](https://github.com/kingdomkind/PubDoots).

##  Licensing
The project's source code is licensed under `LGPL-3.0-or-later`.

The branding (eg. project name, logos etc.) is not covered by the aforementioned license and remains the sole property of `kingdomkind`. Reasonable descriptive use (eg. packaging, articles, etc.) is completely fine.
