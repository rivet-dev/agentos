#include <fenv.h>
#ifdef fesetround
#undef fesetround
#endif
int (*foo)(int) = fesetround;
int main(void) { return 0; }
