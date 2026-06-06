#include <stdio.h>
#ifdef ftello
#undef ftello
#endif
off_t (*foo)(FILE *) = ftello;
int main(void) { return 0; }
