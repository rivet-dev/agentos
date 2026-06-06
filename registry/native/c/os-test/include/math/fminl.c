#include <math.h>
#ifdef fminl
#undef fminl
#endif
long double (*foo)(long double, long double) = fminl;
int main(void) { return 0; }
