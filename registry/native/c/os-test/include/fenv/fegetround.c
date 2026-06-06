#include <fenv.h>
#ifdef fegetround
#undef fegetround
#endif
int (*foo)(void) = fegetround;
int main(void) { return 0; }
