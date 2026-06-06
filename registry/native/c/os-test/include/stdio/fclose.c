#include <stdio.h>
#ifdef fclose
#undef fclose
#endif
int (*foo)(FILE *) = fclose;
int main(void) { return 0; }
