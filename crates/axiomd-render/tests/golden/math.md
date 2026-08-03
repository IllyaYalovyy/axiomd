# Math

Inline math sits in a sentence: the area of a circle is $A = \pi r^2$, and
$e^{i\pi} + 1 = 0$ closes it.

Fractions, roots and nested scripts: $\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}$ and
$x_{i_j}^{2^k}$.

Greek and operators: $\alpha, \beta, \Gamma, \Delta, \sum, \prod, \int, \oint,
\nabla, \partial, \infty, \leq, \geq, \neq, \approx, \in, \subset, \cup, \cap$.

A display equation stands alone:

$$
\int_0^\infty e^{-x^2} \, dx = \frac{\sqrt{\pi}}{2}
$$

A sum with limits above and below:

$$
\sum_{i=1}^{n} i = \frac{n(n+1)}{2}
$$

A matrix:

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{pmatrix} x \\ y \end{pmatrix} =
\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}
$$

An aligned derivation:

$$
\begin{aligned}
(a + b)^2 &= (a + b)(a + b) \\
          &= a^2 + 2ab + b^2
\end{aligned}
$$

A case distinction:

$$
|x| = \begin{cases}
  x  & \text{if } x \geq 0 \\
  -x & \text{otherwise}
\end{cases}
$$

Delimiters that grow with what they hold:

$$
\left( \frac{1}{1 - \frac{1}{n}} \right)^n \xrightarrow{n \to \infty} e
$$

An equation in a heading, which the outline still names:

## The limit $\lim_{n \to \infty} a_n$

And LaTeX nobody can read, which costs the reader the formula and nothing else:
$a^b^c$ here, and a display one below.

$$
\frac{1}
$$

The document goes on.
