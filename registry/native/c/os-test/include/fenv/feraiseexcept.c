#include <fenv.h>
#ifdef feraiseexcept
#undef feraiseexcept
#endif
int (*foo)(int) = feraiseexcept;
int main(void) { return 0; }
