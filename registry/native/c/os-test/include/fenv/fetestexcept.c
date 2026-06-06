#include <fenv.h>
#ifdef fetestexcept
#undef fetestexcept
#endif
int (*foo)(int) = fetestexcept;
int main(void) { return 0; }
