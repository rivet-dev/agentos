#include <fenv.h>
#ifdef feholdexcept
#undef feholdexcept
#endif
int (*foo)(fenv_t *) = feholdexcept;
int main(void) { return 0; }
