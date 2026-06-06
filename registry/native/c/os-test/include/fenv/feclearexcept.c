#include <fenv.h>
#ifdef feclearexcept
#undef feclearexcept
#endif
int (*foo)(int) = feclearexcept;
int main(void) { return 0; }
