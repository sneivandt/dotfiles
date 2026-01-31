#!/bin/sh
set -o errexit
set -o nounset

echo "Comparing base vs arch symlinks"
base_count=$(wc -l < base-symlinks.log)
arch_count=$(wc -l < arch-symlinks.log)
echo "Base profile: $base_count symlinks"
echo "Arch profile: $arch_count symlinks"

if diff -q base-symlinks.log arch-symlinks.log > /dev/null 2>&1; then
  echo "Error: Base and arch profiles link identical files"
  exit 1
fi

# Arch should have more symlinks than base (includes base + arch-specific)
if [ "$arch_count" -le "$base_count" ]; then
  echo "Error: Arch profile should have more symlinks than base"
  exit 1
fi
echo "✓ Base and arch profiles link different files"

echo "Comparing arch vs arch-desktop symlinks"
desktop_count=$(wc -l < arch-desktop-symlinks.log)
echo "Arch-desktop profile: $desktop_count symlinks"

if diff -q arch-symlinks.log arch-desktop-symlinks.log > /dev/null 2>&1; then
  echo "Error: Arch and arch-desktop profiles link identical files"
  exit 1
fi

# Desktop should have more symlinks than arch (includes desktop-specific)
if [ "$desktop_count" -le "$arch_count" ]; then
  echo "Error: Arch-desktop profile should have more symlinks than arch"
  exit 1
fi
echo "✓ Arch and arch-desktop profiles link different files"

# Show some specific differences
echo "Files unique to arch (not in base):"
comm -13 base-symlinks.log arch-symlinks.log | head -5

echo "Files unique to arch-desktop (not in arch):"
comm -13 arch-symlinks.log arch-desktop-symlinks.log | head -5

echo "All profile difference assertions passed!"
