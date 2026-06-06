#include <stdio.h>
#ifdef ftrylockfile
#undef ftrylockfile
#endif
int (*foo)(FILE *) = ftrylockfile;
int main(void) { return 0; }
