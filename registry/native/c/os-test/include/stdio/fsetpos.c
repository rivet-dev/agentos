#include <stdio.h>
#ifdef fsetpos
#undef fsetpos
#endif
int (*foo)(FILE *, const fpos_t *) = fsetpos;
int main(void) { return 0; }
