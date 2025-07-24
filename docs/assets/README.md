# Ferrium Assets

This directory contains visual assets for the Ferrium project.

## 🎨 Logo Files

### Primary Logos
- **`logo.svg`** - Main logo for light backgrounds (3.2KB)
- **`logo-dark.svg`** - Dark theme optimized version (3.4KB)
- **`logo-icon.svg`** - Compact icon version for favicons and small displays (2.5KB)

### Showcase
- **`logo-showcase.html`** - Interactive showcase of all logo variations with usage guidelines

## 📋 Usage

### In README and Documentation
```html
<img src="docs/assets/logo.svg" alt="Ferrium" width="400"/>
```

### Responsive (Light/Dark Theme)
```html
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="docs/assets/logo.svg">
  <img src="docs/assets/logo.svg" alt="Ferrium Logo" width="400"/>
</picture>
```

### For Websites
```html
<!-- Light theme -->
<img src="docs/assets/logo.svg" alt="Ferrium" width="400"/>

<!-- Dark theme -->
<img src="docs/assets/logo-dark.svg" alt="Ferrium" width="400"/>

<!-- Icon/favicon -->
<img src="docs/assets/logo-icon.svg" alt="Ferrium" width="32"/>
```

## 🎨 Design

The logos combine the iron/metal theme (Ferrium = Latin for iron) with distributed systems concepts:
- **Connected nodes** representing distributed architecture
- **Purple-to-pink gradient** for modern tech aesthetic
- **Metal accents** in steel-gray gradients
- **High-contrast subtitle** for readability

## 📁 File Structure

```
docs/assets/
├── README.md           # This file
├── logo.svg           # Main logo (light theme)
├── logo-dark.svg      # Dark theme version
├── logo-icon.svg      # Icon version
└── logo-showcase.html # Interactive showcase
```