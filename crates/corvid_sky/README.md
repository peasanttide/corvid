# `corvid_sky`

Where the sun, the moon and the stars actually were.

Give it an instant and a spot on the earth and it answers where the sun and the
moon are, what phase the moon is in, which stars are up and how bright the air
leaves them. `no_std`, no allocator, no device, and no dependency on any other
crate in this workspace: an ephemeris is arithmetic, and the only thing it needs
from a platform is a sine.

```rust
use corvid_sky::{Civil, Instant, Moon, Observer, Sky, Twilight};

// A crowd on the Paris meridian, at four in the morning on 29 April 1789.
let faubourg = Observer::new(48.8566, 2.3372, 35.0)?;
let seed = Instant::from_civil(Civil::new(1789, 4, 29, 3, 0, 0.0))?;
let four_by_the_sundial = faubourg.apparent_solar_near(seed, 4.0);

let sky = Sky::new(four_by_the_sundial, faubourg);

// The moon set hours ago and is nineteen degrees under the horizon. Whatever
// is lighting that street, it is not the sky.
assert!(sky.moon_position().altitude < -19.0);
assert_eq!(sky.twilight(), Twilight::Nautical);
# Ok::<(), corvid_sky::SkyError>(())
```

## Floating point is correct here, and that is not an oversight

This workspace is fixed point almost everywhere, because a simulation that two
machines have to agree about cannot afford three implementations of rounding.
This crate is the other case, and it is worth stating plainly so that nobody
comes along and "fixes" it.

An ephemeris is transcendental. It is sixty sines summed against a table of
coefficients published in `f64`, a rotation composed of four more, and an
arctangent at the end. Rewriting that in fixed point would not make it
deterministic, it would make it *wrong*, because the error budget of the series
is smaller than the quantisation you would have to impose on it. Nothing here is
hashed, nothing here is sent, and the sky is a picture that two machines are
allowed to disagree about by a millionth of a degree.

What a deterministic simulation does with this is bake it. Evaluate the series
offline over the dates a level covers, emit a table of fixed-point samples with
a record naming the theory and its version, and interpolate that table with
integer arithmetic at run time. Every machine then reads the same bytes and does
the same integer interpolation. This crate is the offline half of that
arrangement and it is deliberately not the other half.

## Two clocks, one set of degrees

Two things to know before the first call, and they are the two that a historical
sky most often gets wrong.

An [`Instant`] carries **two timescales at once**. Terrestrial Time is uniform
and is the argument every series takes; Universal Time tracks the Earth's actual
rotation and is the argument every horizon takes. The gap between them is
delta-T, which is 16.7 seconds in 1789 and 69 seconds today, and using one where
the other belongs turns the whole sky by that much. Nothing here asks a caller to
manage it: build an [`Instant`] from a civil date and both scales are there, with
[`Instant::delta_t`] to show its working.

**Every angle in the public API is in degrees**, right ascension included. An
almanac prints right ascension in hours; this crate does not, because a second
kind of angle in a crate whose whole subject is angles is a conversion waiting
to be forgotten. Fifteen degrees is an hour. Azimuth is measured clockwise from
north, matching a compass and a wind bearing rather than the astronomer's
south-based convention, because one convention across a project beats the right
one in a crate.

```rust
use corvid_sky::{Civil, Instant, Observer, RiseSet};

let site = Observer::new(48.8566, 2.3372, 35.0)?;
let day = Instant::from_civil(Civil::new(1789, 4, 28, 12, 0, 0.0))?;
let sun = RiseSet::sun(&site, day);

// Rise, transit and set are all optional, because at some latitude and some
// date each of them genuinely does not happen.
let noon = sun.transit.ok_or(corvid_sky::SkyError::NotFinite)?;

// The sundial reads twelve at transit, to the second -- which is a joint claim
// about sidereal time, the equation of time and the site's longitude.
assert!((site.apparent_solar(noon) - 12.0).abs() < 1.0 / 3600.0);
# Ok::<(), corvid_sky::SkyError>(())
```

## What is implemented, and where it is published

Nothing here is invented and nothing here is fitted to a screenshot. Each part
is a series somebody printed, and the file that implements it names the page.

The **sun** is Meeus, *Astronomical Algorithms*, 2nd edition, chapter 25, in its
low-accuracy form: mean longitude, mean anomaly, the equation of the centre to
three terms, and the correction from true to apparent longitude. Meeus states
its accuracy as 0.01 degree, which is a twenty-fifth of the sun's own disc.

The **moon** is chapter 47, which is Chapront-Touze and Chapront's ELP-2000/82
truncated to sixty terms in longitude and distance and sixty in latitude, good
to ten arcseconds in longitude and four in latitude. The phase and the
illuminated fraction are chapter 48, solved from the sun-earth-moon triangle
rather than approximated from the elongation. [`Moon::at`] answers a geocentric
position and [`Observer::topocentric`] moves it to the ground, which for the moon
is a correction of nearly a degree.

**Precession** is IAU 2006 in its Fukushima-Williams form -- the four angles of
Capitaine, Wallace and Chapront (2003) -- composed as four frame rotations in
[`frame::to_date`]. This is the big term and the one everybody forgets: the
celestial frame has turned nearly three degrees since J2000, a hundred times what
proper motion does over the same interval. **Nutation** is Meeus's abridged
series at equation 22.1, half an arcsecond.

**Delta-T** is the Espenak and Meeus polynomials from NASA/TP-2006-214141,
piecewise from -1999 to +3000, and before 1600 it is a fit to eclipse records
rather than a model of anything.

**Refraction** is Saemundsson's formula scaled for pressure and temperature,
which gives the standard 34 arcminutes at the horizon. It is stated the way round
that is usable: [`Observer::refraction`] takes a *true* altitude, so a body at
-34 arcminutes is seen exactly on the horizon.

**Rise and set** is not Meeus's three-point interpolation, which quietly loses
accuracy on the moon. [`RiseSet`] scans the day at ten-minute steps and bisects
each crossing against the ephemeris itself: slower, exactly right, and a body
that rises twice in a day or not at all needs no special case.

## The stars are a catalogue rolled back, not a fudge

[`BRIGHT_STARS`] holds **eighty-nine** stars, embedded, so that drawing a sky
needs no download. Every row is transcribed from SIMBAD's record for the star,
whose astrometry is van Leeuwen's re-reduction of the Hipparcos data,
*Astronomy and Astrophysics* 474, 653 (2007), on the ICRS at epoch and equinox
J2000.0, with Johnson `V` and `B` from the same record. A star whose record was
missing any of the ten columns was left out rather than filled in. It is the
naked-eye sky to about magnitude 2.5, plus eight nearby high-proper-motion stars
that make the rollback measurable instead of theoretical.

Getting from that row to 1789 is four corrections, and the famous one is the
smallest. [`Star::propagated`] moves the star through space, keeping the radial
velocity for anything closer than [`Star::NEAR_PARALLAX`] because for those the
change in distance over two centuries bends the apparent path. [`Star::apparent`]
then carries the result through precession and nutation to the true equator and
equinox of date, subtracts the annual parallax and adds the aberration.

```rust
use corvid_sky::{BRIGHT_STARS, Civil, Instant, Star};

let barnard = BRIGHT_STARS.iter().find(|row| row.name == "Barnard's Star").unwrap();
let epoch = Instant::from_terrestrial(Star::EPOCH)?;
let riot = Instant::from_civil(Civil::new(1789, 4, 28, 0, 0, 0.0))?;

// Six tenths of a degree since J2000 -- wider than the moon. The fastest
// proper motion known, and it is a computation and not a constant.
let moved = barnard.travelled(epoch, riot);
assert!((moved - 0.600).abs() < 0.01);

// And it was further away then, because it is approaching at 110 km/s. That
// is why the travel comes out two percent under the catalogued rate times the
// years, rather than exactly equal to it.
assert!(barnard.propagated(riot).parallax < barnard.parallax);
# Ok::<(), corvid_sky::SkyError>(())
```

An engraving wants several thousand stars rather than eighty-nine. A game that
needs them bakes them into an asset with a record naming the catalogue and its
version, the same as any scan; what is embedded here is enough to test the
arithmetic and enough to draw a recognisable sky. [`brighter_than`] takes a
prefix of the table, which is sorted.

## The atmosphere is a medium, not a shader

[`Atmosphere`] is what a sky needs in order to be shaded and is not an opinion
about how. It carries the scattering and absorption coefficients of the air per
colour channel, the density profile to march through, the two phase functions,
the analytic optical depth of the whole column, and the transmittance and
extinction along a path to space. A caller builds lookup tables out of those, or
ray-marches them, or evaluates a closed-form approximation; all three want the
same numbers and none of them belongs in a crate that names no graphics library.

The coefficients are Hillaire's table from *A Scalable and Production Ready Sky
and Atmosphere Rendering Technique* (EGSR 2020), which are Bruneton and Neyret's
fitted to three channels. [`Atmosphere::with_aerosol`] is the one knob a
simulation is expected to drive: a burning building puts a plume into the air,
and a plume is aerosol.

```rust
use corvid_sky::Atmosphere;

let clean = Atmosphere::EARTH;

// Blue is scattered four times as hard as red. That single ratio is why the
// sky is one colour and the sunset is another.
let depth = clean.zenith_optical_depth();
assert!(depth[2] > 3.0 * depth[0]);

// A first-magnitude star setting arrives looking sixth-magnitude, which is why
// stars have to be extinguished toward the horizon rather than merely faded.
assert!(clean.extinction(90.0) < 0.3);
assert!(clean.extinction(0.0) > 4.0);

// And what a riot's smoke column does to the same air.
let plume = clean.with_aerosol(30.0);
assert!(plume.transmittance(30.0)[1] < clean.transmittance(30.0)[1] * 0.7);
```

## Scope

This crate answers **where** and **how bright**, for the sun, the moon and a set
of stars, from any instant and any site on the earth. It converts between the
timescales that question needs, finds rises, sets, transits and twilights, and
hands over the medium the light travels through. It is client ring: nothing in
it is hashed, nothing in it is sent, and two machines are allowed to disagree
about its answers in the last few digits.

It will not draw anything. There is no texture here, no shader, no lookup table
built, no colour: [`Star::colour_index`] is a `B - V` number and turning it into
a chromaticity belongs to whatever owns the palette. It will not model weather,
because cloud is a thing somebody has to read off a page and not a thing an
ephemeris knows. It has no planets yet -- the five naked-eye planets are a
separate set of series and they are not written, and a caller asking for Venus
today gets nothing rather than something invented. It does not model eclipses
beyond what falls out of the positions, and it does not compute illuminance in
lux: turning a solar altitude and a lunar phase into a number of lux is a model
with a citation of its own, and it belongs to the ring that hashes the answer.

Nothing in the source names a city, a date or a game. The historical case this
was built for lives entirely in `tests/reveillon.rs`, where it is arithmetic a
caller does.

## Tests

```sh
cargo test -p corvid_sky --all-features
```

| File | Covers |
|---|---|
| `tests/almanac.rs` | The sun and the moon against Meeus's own worked examples 25 and 47, the IAU 2006 obliquity at its defining epoch, and delta-T in 1789, 2000 and the year 1000 |
| `tests/clock.rs` | Julian day against published anchors including the Gregorian cut, the calendar round trip, every error variant, and the two timescales agreeing in both directions |
| `tests/moon.rs` | A known new moon coming out new and a known full moon full, both to two minutes; the eight phases walking a lunation in order; parallax worth most of a degree |
| `tests/stars.rs` | Barnard's Star moving by the catalogued amount, precession moving the equinox three degrees the right way, and Arcturus and Polaris against an independent implementation |
| `tests/horizon.rs` | Sunrise and sunset bracketing local noon across a year, the twilights in order, 34 arcminutes of refraction, and the atmosphere's phase functions integrating to one |
| `tests/reveillon.rs` | The night of 28-29 April 1789 on the Paris meridian, computed and asserted: the new moon on the 25th, sunset, moonset, and no moon at all after eleven |
| doctests | Every `rust` block on this page and on the types |

The two independent checks are worth naming, because a test written from a
crate's own output proves only that it is consistent. Meeus works two examples
all the way through in the chapters this crate implements, and those are what say
a transcribed table was transcribed correctly -- get one row of the sixty wrong
and the moon's longitude moves. And the star positions in `tests/stars.rs` are
checked against an implementation that shares no code with this one and takes a
different route: Meeus's rigorous zeta-z-theta precession, which is IAU 1976
rather than IAU 2006, with nutation applied as corrections to right ascension and
declination rather than as a rotation. Two models, two formulations, and they
agree to under an arcsecond -- which is the difference between the two precession
models rather than anything either implementation got wrong.
