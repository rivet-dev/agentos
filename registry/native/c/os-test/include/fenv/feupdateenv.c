#include <fenv.h>
#ifdef feupdateenv
#undef feupdateenv
#endif
int (*foo)(const fenv_t *) = feupdateenv;
int main(void) { return 0; }
