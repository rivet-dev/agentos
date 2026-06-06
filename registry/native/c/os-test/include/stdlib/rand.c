#include <stdlib.h>
#ifdef rand
#undef rand
#endif
int (*foo)(void) = rand;
int main(void) { return 0; }
