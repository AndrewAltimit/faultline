/**
 * Interactive tutorial walkthrough for the Faultline simulator.
 *
 * A step-based overlay tour that anchors callout bubbles to UI
 * elements and advances on click or `Enter`. Completely opt-in and
 * self-contained — does not interfere with normal simulator use.
 *
 * Triggered by clicking the "Tutorial" button or programmatically via
 * `Tutorial.start()`. Persists completion in `localStorage` so it
 * does not auto-show repeatedly.
 */

const LS_KEY = 'faultline.tutorial.completed';

const STEPS = [
  {
    title: 'Welcome to Faultline',
    body:
      'This is an analytical conflict simulator. Every scenario is a TOML file describing factions, maps, technologies, and kill chains. The simulator runs Monte Carlo batches and produces feasibility matrices, cost asymmetry ratios, and seam analyses in your browser.',
    target: null,
  },
  {
    title: 'Load a preset',
    body:
      'Pick a bundled scenario here. Try "ETRA 1 — Drone Swarm Decapitation" or "Europe — Eastern Flank" for kill-chain outputs. The TOML below will populate with the scenario definition.',
    target: '#preset-select',
    placement: 'right',
  },
  {
    title: 'TOML editor',
    body:
      'Edit the scenario directly. The Validate button checks structural integrity; Load re-initializes the engine. You can also import/export TOML files or share a URL containing the scenario.',
    target: '#tab-editor',
    placement: 'right',
  },
  {
    title: 'Map view',
    body:
      'Regions are colored by controlling faction. Force units appear as shaped icons. Bundled geographies (US, Europe, East Asia, Middle East) are auto-detected from region IDs; other scenarios fall back to a schematic grid.',
    target: '#map-container',
    placement: 'top',
  },
  {
    title: 'Simulation controls',
    body:
      'Step one tick at a time, play/pause continuously, or scrub the timeline. All runs are deterministic given the same seed.',
    target: '#btn-play',
    placement: 'top',
  },
  {
    title: 'Monte Carlo',
    body:
      'Set the run count and click Run Monte Carlo. The dashboard will show win probabilities, duration distributions, the feasibility matrix for any configured kill chains, cost asymmetry ratios, and doctrinal seam scores.',
    target: '#btn-mc-run',
    placement: 'left',
  },
  {
    title: 'Event log & state inspector',
    body:
      'During a single run, the right panel tracks events as they fire and lets you inspect the full state at each tick. Useful for debugging scenarios or tracing kill-chain progression.',
    target: '#event-log-list',
    placement: 'left',
  },
  {
    title: "You're ready",
    body:
      'Load any preset to begin, or write your own scenario. Scenarios must be derived from publicly available OSINT — see the Docs page for the schema reference and sourcing policy.',
    target: null,
  },
];

export class Tutorial {
  constructor() {
    this.stepIndex = 0;
    this.overlay = null;
    this.bubble = null;
    this.running = false;
  }

  static shouldAutoShow() {
    try {
      return !localStorage.getItem(LS_KEY);
    } catch {
      return false;
    }
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.stepIndex = 0;
    this._buildDom();
    this._renderStep();
  }

  stop() {
    if (!this.running) return;
    this.running = false;
    if (this.overlay) this.overlay.remove();
    this.overlay = null;
    this.bubble = null;
    try {
      localStorage.setItem(LS_KEY, '1');
    } catch {
      /* ignore */
    }
  }

  _buildDom() {
    this.overlay = document.createElement('div');
    this.overlay.className = 'tutorial-overlay';
    this.overlay.innerHTML = `
      <div class="tutorial-backdrop"></div>
      <div class="tutorial-bubble">
        <div class="tutorial-step-count"></div>
        <div class="tutorial-title"></div>
        <div class="tutorial-body"></div>
        <div class="tutorial-actions">
          <button class="tutorial-skip btn-label">Skip tour</button>
          <button class="tutorial-prev btn-label" disabled>Back</button>
          <button class="tutorial-next btn-label">Next</button>
        </div>
      </div>
    `;
    document.body.appendChild(this.overlay);

    this.bubble = this.overlay.querySelector('.tutorial-bubble');
    this.overlay.querySelector('.tutorial-skip').addEventListener('click', () => this.stop());
    this.overlay.querySelector('.tutorial-next').addEventListener('click', () => this._next());
    this.overlay.querySelector('.tutorial-prev').addEventListener('click', () => this._prev());
    // Advance on Enter.
    this._keyHandler = (e) => {
      if (!this.running) return;
      if (e.key === 'Enter') {
        e.preventDefault();
        this._next();
      } else if (e.key === 'Escape') {
        this.stop();
      }
    };
    document.addEventListener('keydown', this._keyHandler);

    this.overlay.addEventListener('remove', () => {
      document.removeEventListener('keydown', this._keyHandler);
    });
  }

  _next() {
    if (this.stepIndex >= STEPS.length - 1) {
      this.stop();
      return;
    }
    this.stepIndex += 1;
    this._renderStep();
  }

  _prev() {
    if (this.stepIndex === 0) return;
    this.stepIndex -= 1;
    this._renderStep();
  }

  _renderStep() {
    const step = STEPS[this.stepIndex];
    const bubble = this.bubble;
    bubble.querySelector('.tutorial-step-count').textContent =
      `Step ${this.stepIndex + 1} of ${STEPS.length}`;
    bubble.querySelector('.tutorial-title').textContent = step.title;
    bubble.querySelector('.tutorial-body').textContent = step.body;
    bubble.querySelector('.tutorial-prev').disabled = this.stepIndex === 0;
    bubble.querySelector('.tutorial-next').textContent =
      this.stepIndex === STEPS.length - 1 ? 'Finish' : 'Next';

    // Position the bubble near the target, if any.
    this._positionBubble(step);
    this._highlightTarget(step);
  }

  _highlightTarget(step) {
    // Remove old highlight class.
    const prev = document.querySelector('.tutorial-target-highlight');
    if (prev) prev.classList.remove('tutorial-target-highlight');

    if (!step.target) return;
    const el = document.querySelector(step.target);
    if (!el) return;
    el.classList.add('tutorial-target-highlight');
    try {
      el.scrollIntoView({ behavior: 'smooth', block: 'center', inline: 'center' });
    } catch {
      /* ignore */
    }
  }

  _positionBubble(step) {
    const bubble = this.bubble;
    // Default: center of screen.
    bubble.style.top = '';
    bubble.style.left = '';
    bubble.style.right = '';
    bubble.style.bottom = '';

    if (!step.target) {
      bubble.style.top = '50%';
      bubble.style.left = '50%';
      bubble.style.transform = 'translate(-50%, -50%)';
      return;
    }
    bubble.style.transform = '';

    const el = document.querySelector(step.target);
    if (!el) {
      bubble.style.top = '50%';
      bubble.style.left = '50%';
      bubble.style.transform = 'translate(-50%, -50%)';
      return;
    }
    const rect = el.getBoundingClientRect();
    const padding = 16;
    const bw = bubble.offsetWidth || 360;
    const bh = bubble.offsetHeight || 200;
    const placement = step.placement || 'bottom';

    let top, left;
    switch (placement) {
      case 'right':
        top = Math.max(padding, rect.top + rect.height / 2 - bh / 2);
        left = Math.min(window.innerWidth - bw - padding, rect.right + padding);
        break;
      case 'left':
        top = Math.max(padding, rect.top + rect.height / 2 - bh / 2);
        left = Math.max(padding, rect.left - bw - padding);
        break;
      case 'top':
        top = Math.max(padding, rect.top - bh - padding);
        left = Math.max(
          padding,
          Math.min(window.innerWidth - bw - padding, rect.left + rect.width / 2 - bw / 2),
        );
        break;
      case 'bottom':
      default:
        top = Math.min(window.innerHeight - bh - padding, rect.bottom + padding);
        left = Math.max(
          padding,
          Math.min(window.innerWidth - bw - padding, rect.left + rect.width / 2 - bw / 2),
        );
        break;
    }
    bubble.style.top = `${top}px`;
    bubble.style.left = `${left}px`;
  }
}
