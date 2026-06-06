#include <fenv.h>
#ifdef fegetenv
#undef fegetenv
#endif
int (*foo)(fenv_t *) = fegetenv;
int main(void) { return 0; }
