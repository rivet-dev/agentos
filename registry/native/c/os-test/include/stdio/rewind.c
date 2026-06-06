#include <stdio.h>
#ifdef rewind
#undef rewind
#endif
void (*foo)(FILE *) = rewind;
int main(void) { return 0; }
