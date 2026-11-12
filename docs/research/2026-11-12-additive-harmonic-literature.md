# HARMONIC: additive synthesis literature survey

2026-11-12. Survey for the HARMONIC engine: up to 5 voices, 48–64 partials per voice, a budget of about 900 cycles per sample per voice on the STM32H750 (Cortex-M7 at 480 MHz, single-precision FPU, 48 kHz). The engine has a chain of 3–4 spectral modules, a Grids-style DENSITY control and an X/Y map.

Evidence tags used on every entry:

- **[R]** I read the source, or the relevant section of it, in this session.
- **[A]** I read only the abstract, the archive metadata or the code header.
- **[S]** Secondary: a manual summary, a review or a third-party write-up.
- **[I]** My inference. No source states it.

The arXiv connector was unavailable for this survey. Sources came from web search and fetch, the DAFx archive API and the authors' own PDFs. No citation below is invented. Where I could not reach the text, the tag says so.

---

## 1. Oscillator banks on small CPUs

**1.1 Vicanek, M. "A New Recursive Quadrature Oscillator." Self-published note, 21 Oct 2015. https://www.vicanek.de/articles/QuadOsc.pdf [R]**
The recursion is three steps per sample: `w = u − k1·v; v += k2·w; u = w − k1·v`, with `k1 = tan(ω/2)` and `k2 = 2k1/(1+k1²)`. That is 3 multiplies and 3 adds, which maps to 3 FMAs. The poles lie exactly on the unit circle even with quantised coefficients, so the oscillator is "hyperstable": it ran for days at 44.1 kHz without drift and needs no renormalisation. By contrast, the coupled form (2-D rotation) "shows systematic deviations already in the first few seconds, and ultimately terminates in an exponential runaway". Changing the frequency moves the state to a new, nearly circular ellipse. Repeated coefficient changes can slowly alter the amplitude, which Vicanek judges "not an issue at all" for music. The appendix compares the alternatives:
- biquad: 1 multiply, poor accuracy at low frequencies;
- Reinsch: good at low frequencies, ideal for static tones;
- digital waveguide oscillator: 1 multiply;
- staggered-update oscillator;
- magic circle: equal amplitudes, almost in quadrature (90° plus half a sample).

→ **Adopt** as the per-partial oscillator wherever partials are not locked to the harmonic series. It needs no renormalisation pass and has quadrature output. The cost is 3 FMAs per partial per sample. The magic circle (2 FMAs) is the cheaper fallback if the budget is tight.

**1.2 Smith, J. O. and Cook, P. R. "The second-order digital waveguide oscillator." ICMC 1992. Discussed on CCRMA PASP, "Digital Sinusoid Generators": https://ccrma.stanford.edu/~jos/pasp/Digital_Sinusoid_Generators.html [S, via JOS page]**
The JOS page sets out three forms: the 2-D rotation (coupled form), the modified coupled form ("magic circle", ε = 2 sin(πfT)) and the direct-form resonator. It says the waveguide oscillator "appears to have the best overall properties yet seen for VLSI implementation".
→ **Consider.** It costs 1 multiply, but Vicanek's table gives it poor low-frequency accuracy. Low partials of bass notes are exactly the hard case for HARMONIC.

**1.3 Plaits harmonic oscillator. Gillet, É. Mutable Instruments Plaits, `plaits/dsp/oscillator/harmonic_oscillator.h` and `dsp/engine/additive_engine.{h,cc}`, MIT licence. https://github.com/pichenettes/eurorack/tree/master/plaits [R, code read]**
Plaits computes one sine per sample with a phase accumulator. It then generates every harmonic with the Chebyshev recursion `T(k+1) = 2x·T(k) − T(k−1)`, which costs one FMA per harmonic, plus one FMA to accumulate. Harmonics are rendered in batches of 12 (`kHarmonicBatchSize`), 36 slots in total: 24 integer harmonics plus a Hammond-drawbar subset. Each harmonic's amplitude is ramped linearly per block through a `ParameterInterpolator`. Band-limiting is a soft taper: `amplitude × (1 − 2f)`, where f is the harmonic's normalised frequency clamped at 0.5. A partial therefore fades out as it nears Nyquist instead of switching off.
→ **Adopt the taper and the block amplitude ramps.** Use the Chebyshev recursion only as a fast path for strictly harmonic spectra, because it cannot express stretch or inharmonicity [I]. The batch-of-12 design keeps the interpolator state in FPU registers [I]. Copying that pattern is the main lesson for the M7. The code is MIT and safe to learn from.

**1.4 Rodet, X. and Depalle, P. "Spectral Envelopes and Inverse FFT Synthesis." 93rd AES Convention, San Francisco, 1992, preprint 3393. https://www.ee.columbia.edu/~dpwe/papers/RodetD92-envs.pdf [R]**
The FFT⁻¹ method writes each partial into a short-time spectrum. With N = 256 and 50 % overlap, a partial occupies about K = 9 significant bins of a tabulated window spectrum (oversampling q ≈ 28). An inverse FFT and overlap-add follow, using a two-window trick that keeps the overlap-add triangular. Measured figures:
- about 80 cycles per partial per frame on a RISC CPU;
- about 200 partials in real time on a MIPS 3000 at 44.1 kHz, with a third of the time spent in the FFT;
- an asymptotic gain of about 30 over oscillators, but a break-even of about L ≈ 20 partials below which oscillators win.

Frame-constant parameters require a symmetric window, so "fast changes in amplitude or frequency force the use of short windows" and reduce the saving. Noise is cheap to inject into any band.
→ **Consider later, avoid for v1.** At 48–64 partials per voice the method sits only a few times past break-even. It costs a frame of latency (about 5 ms at N = 256) and limits attack sharpness, so plucks suffer. Keep it as the route to more than 100 partials or to cheap band-noise.

**1.5 Tondock, H. and von Coler, H. "A Virtual Instrument for IFFT-Based Additive Synthesis in the Ambisonics Domain." DAFx 2023. https://www.dafx.de/paper-archive/2023/DAFx23_paper_26.pdf [R, results section]**
On a modern desktop CPU the IFFT approach beat time-domain synthesis above about 10 partials. At 100 partials it was only about 2× faster (0.239 ms against 0.112 ms per callback). At 1000 partials it was about 6× faster. The authors state that 100 partials cover the deterministic part of most instruments.
→ **Supports the 1.4 verdict.** On a modern CPU the IFFT win at our partial counts is a factor of about 2, not 15. That is not worth the latency and transient smearing for v1.

**1.6 Robine, M., Strandh, R. and Marchand, S. "Fast Additive Sound Synthesis Using Polynomials." DAFx 2006. https://dafx.de/paper-archive/2006/papers/p_181.pdf [R]**
PASS replaces per-sample oscillators with piecewise polynomials scheduled in a priority queue. Its cost scales with the sum of the partial frequencies, so it wins only for low-frequency partials. The paper also benchmarks the digital resonator: 2500 partials for 5 s of 44.1 kHz audio took 3.9 s on a 1.8 GHz Pentium 4. That works out to **about 12.7 cycles per partial-sample** for a desktop recursive bank including overhead (my arithmetic from their table). The paper also reports conflicting prior results: Marchand found the resonator slightly faster than FFT⁻¹, while Meine and Purnhagen (DAFx-02) found FFT⁻¹ fastest. The verdict "highly depends on the implementation details".
→ **Avoid PASS.** Its priority queue and branching do not suit a deterministic audio thread. Use the 12.7 cycles/partial figure as the only published resonator number I found, measured on a desktop.

**1.7 Meine, N. and Purnhagen, H. "Fast Sinusoid Synthesis for MPEG-4 HILN Parametric Audio Decoding." DAFx 2002. https://dafx.de/paper-archive/details/rKKfGpX-M5xmblhkSqMgyA [A]**
The paper compares time-domain methods for constant and linearly swept frequency against frequency-domain methods, reporting execution time and SNR on several CPUs.
→ **Consider** if we ever need a measured comparison with chirped partials (glides). I did not read the full text.

**1.8 Hodes, T. and Freed, A. "Second-order recursive oscillators for musical additive synthesis applications on SIMD and VLIW processors." ICMC 1999. https://cnmat.org/ICMC99/papers/hodes+freed/icmc99-oscill.html [S, search abstract]**
The paper is about error accumulation in recursive oscillators. Direct form I is the cheapest and the most prone to instability at musically useful frequencies. A 16-bit fixed-point variant has its error matched to the range used in additive synthesis.
→ **Avoid direct form I** for low partials. This confirms Vicanek's table.

**1.9 Smith, J. O. "Additive Synthesis." Physical Audio Signal Processing, CCRMA. https://ccrma.stanford.edu/~jos/pasp/Additive_Synthesis.html [S, fetch summary]**
On a 33 MHz DSP56001, about 60 non-interpolated table-lookup partials ran in real time, or about 12 "high-quality" ones. A piano A1 needs about 400 partials. Wavetable access and multiplies are the bottlenecks, and recursive oscillators remove the table.
→ **Context only.** On the M7, a recursion avoids table-lookup latency and flash or cache pressure [I].

**1.10 Roth, J., Keller, D., Castañeda, O. and Studer, C. "Alias-Free Oscillator Synchronization via Additive Synthesis." DAFx 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_49.pdf [R, abstract and implementation]**
Hard and soft sync are done as linear "spectral-resampling" transforms on Fourier coefficients. The cost needed an ASIC (HASY, 65 nm): 512 harmonics, with the transform taking 8,240 clock cycles. The paper references an earlier "big Fourier oscillator" chip from the same group.
→ **Avoid on the M7.** The idea is elegant but far too costly at 512 harmonics. It is worth knowing that sync can be a partial-domain module over a small coefficient set [I].

---

## 2. Spectral envelopes and modifiers

**2.1 Rodet and Depalle 1992 (1.4) [R], spectral-envelope control section.** Controlling hundreds of partials one breakpoint at a time "becomes impossible in practice". The paper's answer is to drive partial amplitudes from a spectral envelope, user-drawn or analysed, sampled at each partial's frequency, with noise described the same way.
→ **Adopt the principle.** Modules should produce or modify an envelope that is sampled at each partial's frequency, at control rate, rather than edit partials one by one.

**2.2 Kawai K5000 additive engine. Sound On Sound review "Kawai K5000R & K5000S", https://www.soundonsound.com/reviews/kawai-k5000r-k5000s [S]; Wikipedia https://en.wikipedia.org/wiki/Kawai_K5000 [S]**
- 64 harmonics per source, two groups of 64, with a looping 5-stage envelope on each harmonic.
- A 128-band formant filter per source, driven by its own envelope or an LFO.
- Harmonics edited in groups labelled "Bright, Dark, Even, Odd". The reviewer singles this out as what makes the engine usable.
- A "MORF" display that builds new harmonic shapes from four sources taken from other patches.
→ **Adopt** grouped controls (odd/even, bright/dark) and a separate formant stage. MORF is commercial precedent for map option B: points that are spectra.

**2.3 Image-Line Harmor manual. https://www.image-line.com/fl-studio-learning/fl-studio-online-manual/html/plugins/Harmor.htm [S, manual summary]**
Harmor has up to 516 partials per voice. Its partial-domain modules:
- **Timbre A/B**, mixed by fade, multiply, min, max and other modes.
- **Blur**, in both the harmonic and the time direction.
- **Prism**, which moves partials off their harmonic positions to sound detuned or metallic.
- **Harmonizer**, which clones partials at octave or offset positions.
- **Filter**, a partial-domain LP/BP/BS/HP with resonance.
- **Phaser**, a moving notch pattern across partials in octave, Hz or harmonic scale. The harmonic scale gives PWM- or FM-like tones.
- **Pluck**, a per-partial decay scaled by frequency so high partials die first.
→ **Adopt** as the best documented module menu. Prism is STRETCH, Phaser is COMB, Pluck is the struck/plucked envelope. Blur maps to a cheap spectral smoother.

**2.4 Plaits additive model, manual: https://pichenettes.github.io/mutable-instruments-documentation/modules/plaits/manual/ [R]**
- HARMONICS is the "number of bumps in the spectrum".
- TIMBRE is the "index of the most prominent harmonic", like a band-pass cutoff.
- MORPH is the "bump shape – from flat and wide to peaked and narrow", like resonance.
- AUX is a Hammond-drawbar subset.
→ **Adopt** as a proven three-knob FORMANT design: centre, width and number of peaks.

**2.5 Grey, J. M. "Multidimensional perceptual scaling of musical timbres." JASA 61, 1270–1277, 1977; Grey, J. M. and Gordon, J. W. "Perceptual effects of spectral modifications on musical timbres." JASA 63, 1493–1500, 1978. [A]**
Grey's 3-D timbre space has three axes:
- spectral energy distribution;
- synchrony of the upper harmonics in the attack;
- low-amplitude, high-frequency attack energy.
Grey and Gordon swapped spectral envelopes between pairs of tones, and the tones moved along the spectral axis as predicted.
→ **Adopt.** The spectral envelope (tilt, brightness) is the axis listeners hear most strongly. Attack synchrony across partials is the second, which argues for per-partial or grouped attacks.

**2.6 McAdams, S., Winsberg, S., Donnadieu, S., De Soete, G. and Krimphoff, J. "Perceptual scaling of synthesized musical timbres: common dimensions, specificities, and latent subject classes." Psychological Research 58(3), 177–192, 1995. PDF: https://three-mode.leidenuniv.nl/pdf/m/mcadams_etal1995pr.pdf [A]**
The common dimensions are log rise time, spectral centroid and degree of spectral variation over time, plus specificities per timbre.
→ **Adopt.** These name the three things a HARMONIC sound should be able to move: attack, brightness and spectral motion.

**2.7 Caclin, A., McAdams, S., Smith, B. K. and Winsberg, S. "Acoustic correlates of timbre space dimensions: a confirmatory study using synthetic tones." JASA 118, 471–482, 2005. doi:10.1121/1.1929229 [A]**
The study used synthetic, additive-style tones. Attack time, spectral centroid and spectrum fine structure (attenuation of even harmonics) were the main determinants of perceived difference. Spectral flux was less salient when other dimensions varied at the same time.
→ **Adopt ODD/EVEN as a first-class module.** It has direct psychoacoustic support as an independent timbre dimension, not just an analogue-square-wave nostalgia control.

**2.8 Saitis, C. and Siedenburg, K. "Brightness perception for musical instrument sounds: relation to timbre dissimilarity and source-cause categories." JASA 2020. PDF: https://comma.eecs.qmul.ac.uk/assets/pdf/2256_1_final_published.pdf [A, abstract read]**
Attack time and spectral centroid are the two most robust correlates of timbre dissimilarity, and brightness behaves as a single perceptual dimension. Centroid is not the whole story: tones with similar centroids can differ in brightness, and faster attacks sound brighter.
→ **Adopt** TILT/BRIGHTNESS as the always-present base control (or the first module). Pair it with attack shape when tuning.

**2.9 Enderby, S. and Stables, R. "A Nonlinear Method for Manipulating Warmth and Brightness." DAFx 2017. https://www.dafx.de/paper-archive/2017/papers/DAFx17_paper_17.pdf [A]**
Warmth and brightness are both strongly correlated with spectral centroids. One parameter driving linear and nonlinear processing let up to 80 % of listeners name the intended descriptor.
→ **Consider.** It supports a single TILT knob with a curved, nonlinear law rather than a linear dB-per-octave mapping.

**2.10 Inharmonicity and stretch.**
- Järveläinen, H., Välimäki, V. and Karjalainen, M. "Audibility of the timbral effects of inharmonicity in stringed instrument tones." Acoustics Research Letters Online 2(3), 79–84, 2001 [S: bibliographic record and a lecture summary; full text blocked]. They measured inharmonicity detection thresholds against f0 and found inharmonicity **easier to hear in low tones**. A 2005 ICMC follow-up found guitar strings always above threshold.
- Rigaud, F., David, B. and Daudet, L. "A Parametric Model of Piano Tuning." DAFx 2011. https://www.dafx.de/paper-archive/2011/Papers/53_e.pdf [A]. Gives a 2-parameter model of the inharmonicity coefficient B across the keyboard.
- Fletcher, Blackham and Stratton, "Quality of Piano Tones." JASA 34, 749–761, 1962 [A]. A 100-oscillator additive synthesiser showed that inharmonicity contributes "warmth".

The standard stiff-string law is fₙ = n·f₀·√(1 + B·n²), usually credited to Fletcher in 1964. I did not read it from a source in this session; one search result suggests the explicit formula is Fletcher 1964 rather than 1962.
→ **Adopt** a STRETCH module with a piano-like law, signed so negative values compress, plus a few modal presets for bars and bells (cf. Karjalainen, Välimäki and Esquef, "Efficient Modeling and Synthesis of Bell-like Sounds", DAFx 2002 [A]). Scale its depth by key so it does not turn bass notes to mud [I, from the low-tone audibility result]. STRETCH is the only module that changes frequencies, which is why the oscillator bank must handle arbitrary per-partial frequencies (1.1).

**2.11 Comb/phaser in the partial domain.** Harmor's Phaser (2.3) [S] is the only documented instance I found. Its harmonic-scale mode gives PWM- or FM-like timbres, which is expected: a periodic notch pattern across harmonic index is the spectrum of a pulse or comb.
→ **Adopt COMB** with spacing in harmonic index, offset (sweep) and depth. It costs one cosine evaluation per partial at control rate, or a table [I]. No peer-reviewed perceptual study found.

---

## 3. Timbre spaces and maps

**3.1 Wessel, D. L. "Timbre Space as a Musical Control Structure." Computer Music Journal 3(2), 45–52, 1979. [A: bibliographic record verified; the CNMAT PDF link returned 404]**
This is the founding proposal: a low-dimensional (2-D) MDS timbre space used as a control space, so that moving through the space produces predictable timbre change, including timbral "transpositions".
→ **Adopt the framing.** The X/Y map should be a perceptual space, not a parameter space.

**3.2 Momeni, A. and Wessel, D. "Characterizing and Controlling Musical Material Intuitively with Geometric Models." NIME 2003, Montreal, 54–62. doi:10.5281/zenodo.1176535. PDF: https://alimomeni.net/files/documents/momeni_interpolation-spaces.pdf [R]**
The interpolation space is built from Gaussian kernels, each tied to a stored vector of numbers, and placed by hand in 2-D. The paper's "Res Space" built a timbre space from a 50-mode timpani resonance model plus eight transformed versions. **It interpolated transformation parameters, not raw partial lists.** Two findings matter here:
1. "interesting musical results are quite often found when exploring the space in between the known regions".
2. With frequency-scaling transforms, naive weights give "a timbre space that is filled with glissandi". Adjustable kernels confine the glissandi to one region.

The paper weighs interpolating raw frequency/amplitude/decay lists, which is option B. It allows completely unrelated models at the points. The costs are that blending and glissando control get harder, and a rich model needs at least 30 components (90 floats), which makes interpolation costlier.
→ **Key evidence for the A/B question.** Their B problems come from interpolating **frequencies** of unrelated models. HARMONIC's partials are index-aligned harmonics, so a B map that stores **amplitudes only** has neither problem [I].

**3.3 Haken, L. Real-time timbre interpolation in Kyma and the Continuum. https://www.hakenaudio.com/addsoundmorph ; Haken, L. "Computational methods for real-time Fourier synthesis." IEEE Trans. Signal Processing 40(9), 2327–2329, 1992 [A]; Tellman, Haken and Holloway, "Timbre morphing of sounds with unequal numbers of features", JAES 1995 [S, cited on Haken's page] [R for the web page]**
Finger x, y and z select a point in a timbre space divided into cubes, with an analysed timbre at each corner. Corner weights are rectilinear so motion stays continuous across cube boundaries. Each partial's amplitude, frequency and noise envelopes are weighted averages of the corners. Figures: 20–160 partials per sound, envelope updates every 128 samples (2.9 ms) with linear interpolation. Pitfalls:
- averaging differently timed attacks blurs them;
- different vibrato rates average into irregular vibrato;
- **partials switching in and out sound "tinkly"**;
- treating noise as many short partials sounds "wormy".
→ **Adopt B mechanics:** bilinear or rectilinear weights over a grid of stored spectra, at control rate. Keep attacks and vibrato out of the stored nodes and supply them from envelopes or modulation. Never hard-gate partials: ramp DENSITY.

**3.4 Horner, A., Beauchamp, J. and Haken, L. "Methods for Multiple Wavetable Synthesis of Musical Instrument Tones." JAES 41(5), 336–356, 1993. https://www.aes.org/e-lib/browse.cfm?elib=7003 [A]; Sandell, G. J. and Martens, W. L. "Perceptual evaluation of principal-component-based synthesis of musical timbres." JAES 43(12), 1013–1028, 1995 [A]**
Three to five fixed basis spectra with time-varying weights usually give a good replica of an instrument tone; the genetic-algorithm choice of spectra beats PCA below four wavetables. Sandell and Martens report near-identical PCA resyntheses at 40–70 % data reduction, extended into a timbre space for interpolating between instruments.
→ **Supports B.** A small set of well-chosen spectra spans a lot of timbre, so a 5×5 = 25-node spectral field is generous rather than limiting [I].

**3.5 Mutable Instruments Grids. Manual: https://pichenettes.github.io/mutable-instruments-documentation/modules/grids/manual/ [R]; code: `grids/pattern_generator.cc`, `grids/resources.cc`, **GPLv3** [R, code read]**
The manual says the map is a collection of drum loops "spatially organized and compressed into a 2-dimensional map" with "machine learning techniques". The code has a 5×5 `drum_map` of 25 nodes. Each node holds 32 steps × 3 instruments of 8-bit **levels**, 96 bytes per node and 2.4 KB in all. `ReadDrumMap` picks the 2×2 cell from the top bits of x and y and does a bilinear `U8Mix`. A step fires when `level > ~density`, so DENSITY is a threshold on the interpolated level. Levels above 192 are accents. Per-pattern random perturbation is added before the threshold (CHAOS).
→ **Adopt the mechanism.** The node stores a strength per element, the map interpolates strengths, and DENSITY thresholds them. Storing 25 nodes × 64 partials × 1 byte is 1.6 KB, matching the ~2 KB budget. **Grids is GPLv3**: reuse the idea only, clean room, and never copy its tables or code (the project needs to stay commercially clean).

**3.6 Esling, P., Chemla-Romeu-Santos, A. and Bitton, A. "Generative timbre spaces: regularizing variational auto-encoders with perceptual metrics." DAFx 2018. arXiv:1805.08501. https://dafx.de/paper-archive/details/Hit9Fiegl7IDPYLADR8a3A [A]**
A VAE latent space is regularised so its distances match MDS timbre spaces. This makes timbre spaces generative and invertible, and the NSGT representation correlated best with them.
→ **Consider for the offline map-building tool.** It is not suitable on-device.

**3.7 Esling, P., Masuda, N., Bardet, A., Despres, R. and Chemla-Romeu-Santos, A. "Universal audio synthesizer control with normalizing flows." DAFx 2019, arXiv:1907.00971; expanded as "Flow Synthesizer", Applied Sciences 10(1), 302, 2020. [A]**
The model learns a latent audio space of a synthesiser and an invertible map to its **parameters**. The latent dimensions act as learned macro-controls. This is option A done by learning.
→ **Avoid for v1.** It is heavy to train, and per-patch learned macros conflict with a 2 KB curated factory map.

**3.8 Limberg, C., Schulz, F., Zhang, Z. and Weinzierl, S. "Pitch-Conditioned Instrument Sound Synthesis From an Interactive Timbre Latent Space." DAFx 2025. https://www.dafx.de/paper-archive/2025/DAFx25_paper_58.pdf [R, method and ablation]**
A 2-D, pitch-disentangled timbre latent space serves as a user interface. The ablation names what a navigable 2-D map needs:
- **without the KL term**, points interpolated between training data sound "less smooth and coherent";
- **without the spread/regularisation loss**, the space becomes lopsided;
- **without the neighbour loss**, the centre crowds while "other regions of the unit circle remain empty, reducing the effectiveness of latent space exploration".
→ **Adopt as design criteria for the map:**
1. Cover the whole square, with no dead zones.
2. Keep neighbouring nodes perceptually close.
3. Keep pitch out of the map (timbre only).

A hand-curated regular 5×5 grid meets the first criterion by construction [I].

**3.9 Natsiou, A., Longo, L. and O'Leary, S. "Interpretable timbre synthesis using variational autoencoders regularized on timbre descriptors." DAFx 2023. https://www.dafx.de/paper-archive/2023/DAFx23_paper_72.pdf [A, plus a skim]**
A VAE over a compact **harmonic-amplitude representation** has latent dimensions regularised to descriptors such as spectral centroid and attack time.
→ **Consider for building the map offline.** Lay out analysed harmonic spectra so that one axis tracks centroid. That makes X/Y legible ("right = brighter").

**3.10 Caetano, M. and Rodet, X. "Musical Instrument Sound Morphing Guided by Perceptually Motivated Features." IEEE TASLP 2013, doi:10.1109/TASL.2013.2260154 [S/A]; Kazazis, S., Depalle, P. and McAdams, S. "Sound Morphing by Audio Descriptors and Parameter Interpolation." DAFx 2016. https://www.dafx.de/paper-archive/2016/dafxpapers/21-DAFx-16_paper_46-PN.pdf [A]**
Linear interpolation of model parameters is not perceptually linear. Interpolating spectral envelopes as line spectral frequencies gave the most linear envelope morphs. Kazazis corrects interpolated spectra toward target descriptor values.
→ **Consider.** Interpolating harmonic amplitudes linearly is physically safe for index-aligned partials (no phase cancellation). It may still feel uneven, so interpolate in a compressed domain (log or square-root amplitude) and check that the centroid moves evenly along an axis [I; verify by host sweep].

**3.11 Loom II (AIR Music Technology).** Morph Pad: per vendor and review material, "up to four variations of your sound" assigned to the quadrants of an X/Y pad. The MusicRadar review, read through a search summary, says each corner stores a snapshot of macros that are "smartly" auto-assigned to spectral, module, effect and modulation parameters. The plug-in has 34 modules and up to 512 partials, and reviewers criticised it for an opaque architecture and thin documentation. [S] I could not verify the full module list or an "Evolver" feature from any source I could reach, so I make no claim about them.
→ **This is option A in commercial form.** Its reviewed weakness is opacity: the user cannot tell what a corner contains.

---

## 4. Partial density, sparsity and noise

**4.1 Lagrange, M. and Marchand, S. "Additive Synthesis of Sound by Taking Advantage of Psychoacoustics." DAFx 2001, Limerick. https://dafx.de/paper-archive/2001/papers/lagrange.pdf [R]**
Partials below the hearing threshold are culled, then masked partials are pruned using a skip-list global mask, recomputed every 16 × 64 samples with under 1 % error.
- **For a single harmonic source "showing poor masking among partials" the algorithm "may seem of little interest"**: it would have been faster to compute everything.
- For three simultaneous sources (voice, sax, guitar), 50 % of the partials were removed and synthesis time almost halved.
- They argue that about 440 partials (50 Hz at 22 kHz) bound almost any sound.
→ **Avoid masking-based pruning per voice.** HARMONIC voices are single harmonic sources, exactly the case where it does not pay. DENSITY should be a **timbre** control, not a CPU saver. Skipping silent partials can still reclaim cycles, but the voice budget must assume the worst case [I].

**4.2 Haken 1992 (3.3) [A].** This paper also prunes perceptually unimportant partials, for large ensembles, and finds that the number of partials needed in chord synthesis reaches a maximum.
→ **Same verdict as 4.1:** pruning pays for ensembles, not for one voice.

**4.3 McAdams, S., Beauchamp, J. W. and Meneguzzi, S. "Discrimination of musical instrument sounds resynthesized with simplified spectrotemporal parameters." JASA 105(2), 882–897, 1999. https://www.mcgill.ca/mpcl/files/mpcl/mcadams_1999_jasa.pdf [R, abstract and setup]**
Seven instruments were resynthesised with six simplifications.
- Discrimination from the original was **very good for spectral-envelope smoothing and for amplitude-envelope coherence** (all partials forced onto one envelope shape).
- It was moderate to poor, in decreasing order, for forced frequency variation, frequency-variation smoothing, **frequency flattening** and amplitude-variation smoothing.
→ **Adopt, with two consequences.**
1. Thinning or smoothing the spectral envelope (DENSITY, blur) is clearly audible. That is good for a musical control, and it means DENSITY steps must be smooth.
2. Partials sharing one envelope is audibly "synthetic". A struck or plucked mode needs per-partial or grouped decay. Per-partial frequency jitter can be skipped cheaply because frequency flattening is hard to hear.

**4.4 Serra, X. and Smith, J. O. "Spectral Modeling Synthesis: A Sound Analysis/Synthesis System Based on a Deterministic plus Stochastic Decomposition." Computer Music Journal 14(4), 12–24, 1990. doi:10.2307/3680788 [A]**
Sound is modelled as sinusoids with time-varying amplitude and frequency, plus a residual of white noise through a time-varying filter.
→ **Adopt the deterministic-plus-stochastic split** for breath and air, with the noise shaped by the same spectral envelope the modules produce.

**4.5 Fitz, K. and Haken, L. Bandwidth-enhanced sinusoidal modelling (Lemur and Loris), ICMC 1995–1996. Cited and summarised on https://www.hakenaudio.com/addsoundmorph [S]**
Noise is folded into the partials by noise-modulating each partial's amplitude. A flute took 481 plain oscillators (38 harmonics plus 443 noise partials) but only 38 bandwidth-enhanced oscillators.
→ **Consider as the cheapest "air".** Multiplying the top N partials' amplitude by low-passed noise costs about one noise sample and one multiply per partial. The result is noise that follows the harmonic envelope, with no FIR filter [I].

**4.6 Engel, J., Hantrakul, L., Gu, C. and Roberts, A. "DDSP: Differentiable Digital Signal Processing." ICLR 2020. arXiv:2001.04643 [R, synth sections via ar5iv]**
The harmonic-plus-noise synthesiser has 100 or 101 harmonics. Amplitudes factor as **A(n)·cₖ(n)**: a global loudness times a harmonic distribution normalised to sum to 1. Noise is uniform noise through a 65-band frequency-domain FIR, with a 64-sample hop.
→ **Adopt the factorisation.** Map nodes and modules work on a *normalised* distribution, and loudness is applied separately. Moving X/Y or DENSITY then changes timbre without lurching level [I]. Whether to renormalise after DENSITY is a voicing choice for the owner's ear.

---

## 5. Partial envelopes and time evolution

**5.1 K5000 (2.2) [S]:** an envelope on every harmonic, with grouped editing as the usable layer over it. **Harmor Pluck (2.3) [S]:** one knob sets decay time against partial frequency, so high partials die first. **McAdams et al. 1999 (4.3) [R]:** envelope coherence is very discriminable.
→ **Adopt** a grouped decay law: one decay time plus a "high partials faster" slope, applied per partial at control rate. It costs one multiply per partial per control block. It is the cheapest way out of the audible single-envelope coherence, and it gives struck and plucked sounds.

**5.2 Haken (3.3) [R]:** envelope parameter streams at 128-sample blocks with linear interpolation are enough for expressive real-time morphing.
→ **Adopt** a 32–64-sample control block for the module chain and map: about 0.7–1.3 ms at 48 kHz. Amplitudes ramp linearly inside the block, as in Plaits.

**5.3 Spectral motion.** McAdams et al. 1995 (2.6) [A] lists the degree of spectral variation as a common timbre dimension.
→ **Consider** per-module LFO or envelope targets (formant centre, comb offset, map X/Y) as the "evolving" layer. I found no peer-reviewed description of Loom II's evolution features, so this rests on the perceptual result only.

---

## 6. Recent work (2018–2026)

**6.1 Hayes, B., Shier, J., Fazekas, G., McPherson, A. and Saitis, C. "A review of differentiable digital signal processing for music and speech synthesis." Frontiers in Signal Processing, 11 Jan 2024. doi:10.3389/frsip.2023.1284100; arXiv:2308.15422 [A]**
This is the survey of DDSP: harmonic-plus-noise, sound matching and optimisation pathologies, with a companion web book at https://intro2ddsp.github.io/.
→ **Consider** if we build an offline "analyse a sound into map nodes" tool.

**6.2 Giudici, G. et al. "Distilling DDSP: Exploring Real-Time Audio Generation on Embedded Systems." JAES 73(6), 2025 (Sound of DAFx special issue). https://gregogiudici.github.io/distilling-ddsp [A]**
Knowledge distillation shrinks DDSP models (harmonic-plus-noise, FM, wavetable) by up to about 100× for embedded inference. The project page gives no hardware or cycle figures.
→ **Avoid on-device neural control for 0.4.** A neural decoder per voice would eat the budget the partials need [I]. It is fine offline.

**6.3 Ganis, F., Knudsen, E. F., Lyster, S. V. K., Otterbein, R., Südholt, D. and Erkut, C. "Real-time Timbre Transfer and Sound Synthesis using DDSP." SMC 2021. arXiv:2103.07220 [A]**
This is a JUCE plug-in implementation of DDSP. Harmonics above Nyquist are removed.
→ **Context.** It confirms that Nyquist gating is standard. Prefer the Plaits soft taper (1.3) to avoid pops when pitch bends partials across Nyquist.

**6.4 Roth et al. DAFx 2026 (1.10) [R] and Tondock and von Coler DAFx 2023 (1.5) [R].** The only recent DAFx additive-synthesiser papers I found. The DAFx archive API search for "additive" returned nothing on additive *instrument design* or embedded oscillator banks in 2018–2026.
→ **Gap.** There is no recent peer-reviewed reference design for embedded additive instruments. Our nearest proven precedents are Plaits (code) and the K5000 (product).

**6.5 Weingartner, W. E. "Stable Limit Cycles as Tunable Signal Sources." DAFx 2025 [A].** Nonlinear oscillators (van der Pol, Brusselator) with frequency and amplitude decoupled from their internal parameters.
→ **Avoid for partials.** It is relevant only as a possible exotic source module.

Dropped: I did not find, and so do not cite, a peer-reviewed paper on "Grids-like learned maps for timbre" or on the Loom II Evolver.

---

## Synthesis

### (a) Oscillator method and partial count on the M7

- **Method: a per-partial recursive bank in the time domain.** Use the Vicanek quadrature oscillator (3 FMAs) or the magic circle (2 FMAs), with the loop ordered **partial-outer, sample-inner** over a 32–64-sample block. Oscillator state then lives in registers, and the block accumulator stays in DTCM [I]. The module chain, map and DENSITY run once per block (Haken's 2.9 ms streams [R], Plaits' block ramps [R]). Amplitudes ramp linearly per sample, and the Plaits `(1 − 2f)` taper handles band-limiting [R]. The Chebyshev harmonic recursion (Plaits [R]) is an optional fast path only when STRETCH = 0.
- **Numbers.**
  - Published: about 12.7 cycles per partial-sample for a recursive bank on a Pentium 4, including overhead (Robine 2006 [R], my arithmetic). FFT⁻¹ breaks even near 20 partials (Rodet 1992 [R]). On a desktop it was only about 2× faster at 100 partials (Tondock 2023 [R]).
  - My estimate for the M7: about 6–9 cycles per partial-sample for the recursion, the FMA into the accumulator and the amplitude ramp [I, **must be measured on the unit**]. That puts 48 partials at about 300–430 cycles and 64 at about 400–580, leaving 300+ of the ~900 budget for the control-rate chain, noise and envelopes.
- **Count.** 64 partials covers the full band only for f0 ≥ 375 Hz at 48 kHz. At 50 Hz, about 440 would be needed (Lagrange 2001 [R]). The K5000 used 64 per source [S], Plaits 24 + 12 slots [R], and Tondock says about 100 cover most instruments [R].
- **Recommendation.** Take **48 as the guaranteed count and 64 where the measured budget allows.** Add noise-modulated top partials (Fitz and Haken [S]) for air above the last partial on bass notes. Do not use masking-based pruning: it does not pay for a single harmonic source (Lagrange 2001 [R]). Keep IFFT synthesis as the documented upgrade path if more than 100 partials are ever needed.

### (b) The most perceptually distinct modules (8)

Each is ranked by the perceptual dimension it moves.
1. **TILT/BRIGHTNESS**: spectral centroid, the most robust dimension (Grey 1977 [A], McAdams 1995 [A], Caclin 2005 [A], Saitis 2020 [A]).
2. **ODD/EVEN**: spectral fine structure, confirmed independent (Caclin 2005 [A]; K5000 groups [S]).
3. **FORMANT**: spectral-envelope peaks with centre, width and count (Rodet and Depalle [R], Plaits [R], K5000 [S], Grey and Gordon 1978 [A]).
4. **STRETCH/INHARMONICITY**: piano law plus bar and bell presets, depth scaled by key (Järveläinen 2001 [S], Rigaud 2011 [A], Harmor Prism [S]).
5. **COMB/PHASER** over harmonic index, which gives pulse/PWM-like notches (Harmor [S]; no perceptual paper).
6. **DENSITY/BLUR**: thinning and smoothing the envelope, which is very discriminable (McAdams et al. 1999 [R]; Grids mechanism [R]).
7. **PLUCK/DECAY SLOPE**: per-partial decay against frequency, which breaks the audible single-envelope coherence (McAdams 1999 [R], Harmor Pluck [S], K5000 [S]).
8. **AIR/NOISE**: deterministic plus stochastic (Serra and Smith 1990 [A], DDSP [R], bandwidth-enhanced partials [S]).

DENSITY and AIR could be fixed controls rather than chain slots, leaving TILT, ODD/EVEN, FORMANT, STRETCH, COMB and PLUCK as the chain menu [I].

### (c) The map: A (module settings) or B (a field of spectra)

**The literature favours B, with frequencies kept out of the map.**
- Haken's corner-weighted spectra [R], the K5000 MORF [S] and Horner's 3–5-basis-spectra result [A] all show that interpolating stored spectra is musical.
- The classic objections to B are partial correspondence, glissandi and cost (Momeni and Wessel 2003 [R]; Tellman, Haken and Holloway [S]). They arise from interpolating the **frequencies** of unrelated analyses. HARMONIC's partials are index-aligned, so a B map storing only 8-bit log amplitudes (25 × 64 = 1.6 KB) avoids all three. Nodes are mixed bilinearly at control rate, and DENSITY thresholds the interpolated strength as in Grids [R, mechanism] [I, applicability].
- Option A (Loom II) puts opaque macro snapshots under the pad and was criticised for exactly that [S]. A also lets one knob position move every module at once, which makes the module knobs untrustworthy [I].
- Learned-map work (Limberg 2025 [R], Esling 2018 [A]) says a navigable 2-D map needs even coverage, close neighbours and no pitch. A curated regular 5×5 grid gives the first by construction. The other two are curation rules: lay nodes out so X tracks brightness and Y tracks odd/even or formant character, and keep all pitch and frequency behaviour in the modules.

**Recommendation: B, as a 5×5 spectral field.** The map supplies the source spectrum. The modules (post-map) shape it, and their settings stay on their own knobs. DENSITY thresholds per-partial strength from the map. Build the field offline from analyses with clean provenance, ordered by descriptor (Natsiou 2023 [A]). Interpolate in a compressed amplitude domain, with a host sweep to check the centroid moves evenly (Caetano and Rodet [S], Kazazis 2016 [A]) [I]. Ramp threshold crossings over a block, never hard-gate (Haken's "tinkly" [R]). Per Grids' GPLv3 licence, copy no Grids code or data.

### (d) Top 5 reading list for the implementer

1. Vicanek 2015, "A New Recursive Quadrature Oscillator" (4 pages): the oscillator and its stability.
2. Plaits `harmonic_oscillator.h` and `additive_engine.{h,cc}` (MIT): block ramps, the Nyquist taper, register-batched harmonics and formant-bump controls on a Cortex-M.
3. Grids `pattern_generator.cc` (`ReadDrumMap`, `EvaluateDrums`): map interpolation and density threshold. Read for the idea only (GPLv3).
4. McAdams, Beauchamp and Meneguzzi 1999, JASA: what is and is not audible in harmonic spectra, which decides where cycles go.
5. Momeni and Wessel 2003, NIME: interpolation spaces, and the trade-off between parameter-point and spectrum-point maps.
   - Runner-up: Rodet and Depalle 1992, for spectral-envelope control and the IFFT upgrade path.
