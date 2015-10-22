# Dotfiles

This is a project to store configuration files for various Linux applications. The provided installation script will create symlinks in a users $HOME directory.

The files which will be effected can be seen in [links](links).

Be warned that existing dotfiles may be overridden by installing this configuration.

### Configure

If you want to ignore a subset of the symlinks, list them in a .linkignore file.

### Install

Update dependencies and create symlinks in $HOME. This includes updating the vim plugins managed by [vim-plug](https://github.com/junegunn/vim-plug).

    make

### Uninstall

Remove all the symlinks created in $HOME. Note that the uninstall process will leave behind directories in you home directory that contained symlinks to ensure that other files, not managed by this project, are not also removed.

    make uninstall

### Running as root

This instalation will potentially override many files in the users $HOME. The installation will not proceed if run as root to protect the root configuration. If you would like to ignore this warning you can run the following command as root to do the installation.

    make allow_root=yes
