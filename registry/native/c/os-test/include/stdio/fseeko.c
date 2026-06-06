#include <stdio.h>
#ifdef fseeko
#undef fseeko
#endif
int (*foo)(FILE *, off_t, int) = fseeko;
int main(void) { return 0; }
