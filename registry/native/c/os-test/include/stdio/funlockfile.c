#include <stdio.h>
#ifdef funlockfile
#undef funlockfile
#endif
void (*foo)(FILE *) = funlockfile;
int main(void) { return 0; }
