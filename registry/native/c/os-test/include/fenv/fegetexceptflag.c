#include <fenv.h>
#ifdef fegetexceptflag
#undef fegetexceptflag
#endif
int (*foo)(fexcept_t *, int) = fegetexceptflag;
int main(void) { return 0; }
