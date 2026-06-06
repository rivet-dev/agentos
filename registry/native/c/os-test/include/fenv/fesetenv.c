#include <fenv.h>
#ifdef fesetenv
#undef fesetenv
#endif
int (*foo)(const fenv_t *) = fesetenv;
int main(void) { return 0; }
