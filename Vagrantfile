# vi: set ft=ruby :

Vagrant.configure("2") do |config|
  config.vm.define "dotfiles" do |v|
    v.vm.provider "docker" do |d|
      d.name = "dotfiles"
      d.build_dir = "."
      d.has_ssh = true
      d.build_args = ["--tag=vagrant/dotfiles"]
    end
    v.ssh.username = "root"
    v.ssh.password = "root"
  end
end
