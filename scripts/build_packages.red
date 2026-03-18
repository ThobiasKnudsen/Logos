% build_packages.red — Compile REDUCE packages into the image.
%
% Each sub-module of a package needs its own FASL entry in the image.
% When load_package is called at runtime, it loads the header module,
% which calls create!-package to get the sub-module list, then loads
% each sub-module by name.

symbolic;

% ── Search paths for file lookups ──
if not boundp 'loaddirectories!* then loaddirectories!* := nil;

loaddirectories!* := append(loaddirectories!*, '(
  "vendor/reduce-packages/factor/"
  "vendor/reduce-packages/int/"
  "vendor/reduce-packages/solve/"
  "vendor/reduce-packages/matrix/"
  "vendor/reduce-packages/taylor/"
  "vendor/reduce-packages/misc/"
  "vendor/reduce-packages/limit/"
));

prin2t "=== Starting package compilation ===";

% ═══════════════════════════════════════════════════════════════════════
% 1. EZGCD — polynomial GCD (in factor/ directory)
%    Modules: ezgcd alphas coeffts ezgcdf facmisc facstr interfac
%             linmodp mhensfns modpoly multihen unihens
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling ezgcd ===";

faslout 'ezgcd;
in "vendor/reduce-packages/factor/ezgcd.red"$
faslend;

faslout 'alphas;
in "vendor/reduce-packages/factor/alphas.red"$
faslend;

faslout 'coeffts;
in "vendor/reduce-packages/factor/coeffts.red"$
faslend;

faslout 'ezgcdf;
in "vendor/reduce-packages/factor/ezgcdf.red"$
faslend;

faslout 'facmisc;
in "vendor/reduce-packages/factor/facmisc.red"$
faslend;

faslout 'facstr;
in "vendor/reduce-packages/factor/facstr.red"$
faslend;

faslout 'interfac;
in "vendor/reduce-packages/factor/interfac.red"$
faslend;

faslout 'linmodp;
in "vendor/reduce-packages/factor/linmodp.red"$
faslend;

faslout 'mhensfns;
in "vendor/reduce-packages/factor/mhensfns.red"$
faslend;

faslout 'modpoly;
in "vendor/reduce-packages/factor/modpoly.red"$
faslend;

faslout 'multihen;
in "vendor/reduce-packages/factor/multihen.red"$
faslend;

faslout 'unihens;
in "vendor/reduce-packages/factor/unihens.red"$
faslend;

prin2t "=== ezgcd done ===";

% ═══════════════════════════════════════════════════════════════════════
% 2. FACTOR — polynomial factorization (depends on ezgcd)
%    Modules: factor bigmodp degsets facprim facmod facuni
%             imageset pfactor vecpoly pfacmult
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling factor ===";

faslout 'factor;
in "vendor/reduce-packages/factor/factor.red"$
faslend;

faslout 'bigmodp;
in "vendor/reduce-packages/factor/bigmodp.red"$
faslend;

faslout 'degsets;
in "vendor/reduce-packages/factor/degsets.red"$
faslend;

faslout 'facprim;
in "vendor/reduce-packages/factor/facprim.red"$
faslend;

faslout 'facmod;
in "vendor/reduce-packages/factor/facmod.red"$
faslend;

faslout 'facuni;
in "vendor/reduce-packages/factor/facuni.red"$
faslend;

faslout 'imageset;
in "vendor/reduce-packages/factor/imageset.red"$
faslend;

faslout 'pfactor;
in "vendor/reduce-packages/factor/pfactor.red"$
faslend;

faslout 'vecpoly;
in "vendor/reduce-packages/factor/vecpoly.red"$
faslend;

faslout 'pfacmult;
in "vendor/reduce-packages/factor/pfacmult.red"$
faslend;

prin2t "=== factor done ===";

% ═══════════════════════════════════════════════════════════════════════
% 3. MATRIX — matrix operations
%    Modules: matrix matsm matpri matproc extops bareiss det glmat
%             nullsp rank nestdom resultnt brsltnt_bmap cofactor
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling matrix ===";

faslout 'matrix;
in "vendor/reduce-packages/matrix/matrix.red"$
faslend;

faslout 'matsm;
in "vendor/reduce-packages/matrix/matsm.red"$
faslend;

faslout 'matpri;
in "vendor/reduce-packages/matrix/matpri.red"$
faslend;

faslout 'matproc;
in "vendor/reduce-packages/matrix/matproc.red"$
faslend;

faslout 'extops;
in "vendor/reduce-packages/matrix/extops.red"$
faslend;

faslout 'bareiss;
in "vendor/reduce-packages/matrix/bareiss.red"$
faslend;

faslout 'det;
in "vendor/reduce-packages/matrix/det.red"$
faslend;

faslout 'glmat;
in "vendor/reduce-packages/matrix/glmat.red"$
faslend;

faslout 'nullsp;
in "vendor/reduce-packages/matrix/nullsp.red"$
faslend;

faslout 'rank;
in "vendor/reduce-packages/matrix/rank.red"$
faslend;

faslout 'nestdom;
in "vendor/reduce-packages/matrix/nestdom.red"$
faslend;

faslout 'resultnt;
in "vendor/reduce-packages/matrix/resultnt.red"$
faslend;

faslout 'brsltnt_bmap;
in "vendor/reduce-packages/matrix/brsltnt_bmap.red"$
faslend;

faslout 'cofactor;
in "vendor/reduce-packages/matrix/cofactor.red"$
faslend;

prin2t "=== matrix done ===";

% ═══════════════════════════════════════════════════════════════════════
% 4. SOLVE — equation solving (depends on matrix)
%    Modules: solve solve1 ppsoln solvelnr glsolve solvealg
%             solvetab quartic
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling solve ===";

faslout 'solve;
in "vendor/reduce-packages/solve/solve.red"$
faslend;

faslout 'solve1;
in "vendor/reduce-packages/solve/solve1.red"$
faslend;

faslout 'ppsoln;
in "vendor/reduce-packages/solve/ppsoln.red"$
faslend;

faslout 'solvelnr;
in "vendor/reduce-packages/solve/solvelnr.red"$
faslend;

faslout 'glsolve;
in "vendor/reduce-packages/solve/glsolve.red"$
faslend;

faslout 'solvealg;
in "vendor/reduce-packages/solve/solvealg.red"$
faslend;

faslout 'solvetab;
in "vendor/reduce-packages/solve/solvetab.red"$
faslend;

faslout 'quartic;
in "vendor/reduce-packages/solve/quartic.red"$
faslend;

prin2t "=== solve done ===";

% ═══════════════════════════════════════════════════════════════════════
% 5. INT — integration (depends on solve, factor)
%    Modules: int contents csolve idepend df2q distrib divide driver
%             symint intfac ibasics makevars jpatches reform simpsqrt
%             hacksqrt sqrtf isolve tidysqrt trcase halfangl trialdiv
%             vect dint
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling int ===";

faslout 'int;
in "vendor/reduce-packages/int/int.red"$
faslend;

faslout 'contents;
in "vendor/reduce-packages/int/contents.red"$
faslend;

faslout 'csolve;
in "vendor/reduce-packages/int/csolve.red"$
faslend;

faslout 'idepend;
in "vendor/reduce-packages/int/idepend.red"$
faslend;

faslout 'df2q;
in "vendor/reduce-packages/int/df2q.red"$
faslend;

faslout 'distrib;
in "vendor/reduce-packages/int/distrib.red"$
faslend;

faslout 'divide;
in "vendor/reduce-packages/int/divide.red"$
faslend;

faslout 'driver;
in "vendor/reduce-packages/int/driver.red"$
faslend;

faslout 'symint;
in "vendor/reduce-packages/int/symint.red"$
faslend;

faslout 'intfac;
in "vendor/reduce-packages/int/intfac.red"$
faslend;

faslout 'ibasics;
in "vendor/reduce-packages/int/ibasics.red"$
faslend;

faslout 'makevars;
in "vendor/reduce-packages/int/makevars.red"$
faslend;

faslout 'jpatches;
in "vendor/reduce-packages/int/jpatches.red"$
faslend;

faslout 'reform;
in "vendor/reduce-packages/int/reform.red"$
faslend;

faslout 'simpsqrt;
in "vendor/reduce-packages/int/simpsqrt.red"$
faslend;

faslout 'hacksqrt;
in "vendor/reduce-packages/int/hacksqrt.red"$
faslend;

faslout 'sqrtf;
in "vendor/reduce-packages/int/sqrtf.red"$
faslend;

faslout 'isolve;
in "vendor/reduce-packages/int/isolve.red"$
faslend;

faslout 'tidysqrt;
in "vendor/reduce-packages/int/tidysqrt.red"$
faslend;

faslout 'trcase;
in "vendor/reduce-packages/int/trcase.red"$
faslend;

faslout 'halfangl;
in "vendor/reduce-packages/int/halfangl.red"$
faslend;

faslout 'trialdiv;
in "vendor/reduce-packages/int/trialdiv.red"$
faslend;

faslout 'vect;
in "vendor/reduce-packages/int/vect.red"$
faslend;

faslout 'dint;
in "vendor/reduce-packages/int/dint.red"$
faslend;

prin2t "=== int done ===";

% ═══════════════════════════════════════════════════════════════════════
% 6. TAYLOR — Taylor series expansion
%    Modules: taylor tayintro tayutils tayintrf tayexpnd taybasic
%             taysimp taysubst taydiff tayconv tayprint tayfront
%             tayfns tayrevrt tayimpl taypart taygamma
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling taylor ===";

faslout 'taylor;
in "vendor/reduce-packages/taylor/taylor.red"$
faslend;

faslout 'tayintro;
in "vendor/reduce-packages/taylor/tayintro.red"$
faslend;

faslout 'tayutils;
in "vendor/reduce-packages/taylor/tayutils.red"$
faslend;

faslout 'tayintrf;
in "vendor/reduce-packages/taylor/tayintrf.red"$
faslend;

faslout 'tayexpnd;
in "vendor/reduce-packages/taylor/tayexpnd.red"$
faslend;

faslout 'taybasic;
in "vendor/reduce-packages/taylor/taybasic.red"$
faslend;

faslout 'taysimp;
in "vendor/reduce-packages/taylor/taysimp.red"$
faslend;

faslout 'taysubst;
in "vendor/reduce-packages/taylor/taysubst.red"$
faslend;

faslout 'taydiff;
in "vendor/reduce-packages/taylor/taydiff.red"$
faslend;

faslout 'tayconv;
in "vendor/reduce-packages/taylor/tayconv.red"$
faslend;

faslout 'tayprint;
in "vendor/reduce-packages/taylor/tayprint.red"$
faslend;

faslout 'tayfront;
in "vendor/reduce-packages/taylor/tayfront.red"$
faslend;

faslout 'tayfns;
in "vendor/reduce-packages/taylor/tayfns.red"$
faslend;

faslout 'tayrevrt;
in "vendor/reduce-packages/taylor/tayrevrt.red"$
faslend;

faslout 'tayimpl;
in "vendor/reduce-packages/taylor/tayimpl.red"$
faslend;

faslout 'taypart;
in "vendor/reduce-packages/taylor/taypart.red"$
faslend;

faslout 'taygamma;
in "vendor/reduce-packages/taylor/taygamma.red"$
faslend;

prin2t "=== taylor done ===";

% ═══════════════════════════════════════════════════════════════════════
% 7. MISC — miscellaneous utilities
% ═══════════════════════════════════════════════════════════════════════
prin2t "=== Compiling misc ===";

faslout 'misc;
in "vendor/reduce-packages/misc/misc.red"$
faslend;

prin2t "=== misc done ===";

% NOTE: limits package skipped — requires TPS (Truncated Power Series)

% ═══════════════════════════════════════════════════════════════════════
% ALL PACKAGES COMPILED — now preserve the image
% ═══════════════════════════════════════════════════════════════════════

prin2t "=== All packages compiled, preserving image ===";

% Args: (restart_function, banner_string)
% 'begin is the standard REDUCE restart function (enters algebraic mode).
preserve('begin, "Logos REDUCE");

end;
