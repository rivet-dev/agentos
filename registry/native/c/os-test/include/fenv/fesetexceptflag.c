#include <fenv.h>
#ifdef fesetexceptflag
#undef fesetexceptflag
#endif
int (*foo)(const fexcept_t *, int) = fesetexceptflag;
int main(void) { return 0; }
