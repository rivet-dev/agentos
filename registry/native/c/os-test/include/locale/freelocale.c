#include <locale.h>
#ifdef freelocale
#undef freelocale
#endif
void (*foo)(locale_t) = freelocale;
int main(void) { return 0; }
