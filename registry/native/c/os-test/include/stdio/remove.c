#include <stdio.h>
#ifdef remove
#undef remove
#endif
int (*foo)(const char *) = remove;
int main(void) { return 0; }
