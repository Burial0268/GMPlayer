import { BaseRenderer } from "@applemusic-like-lyrics/core";
import { GLProgram, createOffscreenCanvas, GLTexture } from "./GLProgram";
import { vec2 } from "gl-matrix";
import { vertexShader as mainVertexShader } from "./glsl/duplicate.vert";
import { fragmentShader as mainFragmentShader } from "./glsl/duplicate.frag";
import { fragmentShader as flowMapFragmentShader } from "./glsl/flowmap.frag";

export interface AppleBackgroundRenderOptions {
    rotationSpeed?: number;
    saturation?: number;
    blurLevel?: number;
    flowStrength?: number;
    distortionStrength?: number;
    brightness?: number;
    baseColor?: [number, number, number];
}

export class FbmRenderer extends BaseRenderer {
    private gl: WebGL2RenderingContext;
	private frameTime = 0;
	private currentImageData?: ImageData;
	private lastTickTime = 0;
	private tickHandle = 0;
	private maxFPS = 60;
	private isPaused = false;
    private isStatic = false;
	private mainProgram: GLProgram;
    private flowMapProgram: GLProgram;
	private currentSize = vec2.fromValues(0, 0);
	private _disposed = false;
	private texture?: GLTexture;
	private flowMapTexture?: GLTexture;
    private flowMapFramebuffer?: WebGLFramebuffer;
	private positionBuffer?: WebGLBuffer;
	private texCoordBuffer?: WebGLBuffer;
	private indexBuffer?: WebGLBuffer;
	private isNoCover = true;
	private baseFlowSpeed = 0.4;
	private timeMultiplier = 0.4;
    private volumeFactor = 1;
	private saturation = 1.0;
	private blurLevel = 6.0;
	private hasLyric = false;
	private renderScale = 0.5;
    private flowStrength = 0.2;
    private distortionStrength = 0.12;
    private brightness = 1.28;
    private baseColor: [number, number, number] = [0.5, 0.5, 0.5];

	setFlowSpeed(speed: number) {
		this.baseFlowSpeed = speed;
        this.timeMultiplier = this.baseFlowSpeed * this.volumeFactor;
	}

	setSaturation(saturation: number) {
		this.saturation = Math.max(0, Math.min(1, saturation));
	}

	setBlur(level: number) {
		this.blurLevel = Math.max(0, level);
	}

	setRenderScale(scale: number) {
		this.renderScale = Math.max(0.1, Math.min(1, scale));
		this.updateFramebufferSize();
	}

    private computeDominantColor(imageData: ImageData): [number, number, number] {
        const data = imageData.data;
        const totalPixels = data.length / 4;
        const targetSamples = 5000;
        const stepPx = Math.max(1, Math.floor(totalPixels / targetSamples));
        const step = stepPx * 4;

        let sumR = 0, sumG = 0, sumB = 0, count = 0;
        let brightR = 0, brightG = 0, brightB = 0, brightCount = 0;
        let bestSat = -1;
        let best: [number, number, number] = [0.5, 0.5, 0.5];

        for (let i = 0; i < data.length; i += step) {
            const r = data[i] / 255;
            const g = data[i + 1] / 255;
            const b = data[i + 2] / 255;
            const luma = 0.299 * r + 0.587 * g + 0.114 * b;
            if (luma < 0.08 || luma > 0.98) continue;

            sumR += r; sumG += g; sumB += b; count++;
            if (luma > 0.32) {
                brightR += r; brightG += g; brightB += b; brightCount++;
            }

            const maxc = Math.max(r, g, b);
            const minc = Math.min(r, g, b);
            const chroma = maxc - minc;
            const sat = chroma / (maxc + 1e-5);
            if (sat > bestSat) {
                bestSat = sat;
                best = [r, g, b];
            }
        }

        if (count === 0) return this.baseColor;
        const avg: [number, number, number] = [sumR / count, sumG / count, sumB / count];
        const brightAvg: [number, number, number] = brightCount > 0 ? [brightR / brightCount, brightG / brightCount, brightB / brightCount] : avg;
        const candidate = bestSat < 0.05 ? avg : [
            best[0] * 0.6 + avg[0] * 0.4,
            best[1] * 0.6 + avg[1] * 0.4,
            best[2] * 0.6 + avg[2] * 0.4,
        ];
        const blended: [number, number, number] = [
            candidate[0] * 0.55 + brightAvg[0] * 0.45,
            candidate[1] * 0.55 + brightAvg[1] * 0.45,
            candidate[2] * 0.55 + brightAvg[2] * 0.45,
        ];
        // Enforce a minimum luminance to avoid过暗背景
        const l = 0.299 * blended[0] + 0.587 * blended[1] + 0.114 * blended[2];
        const lift = l < 0.32 ? (0.32 - l) : 0;
        return [
            Math.min(1, blended[0] + lift),
            Math.min(1, blended[1] + lift),
            Math.min(1, blended[2] + lift),
        ];
    }

	// This method is kept for API consistency but is no longer used by the metaball shader.
	setWarp(amount: number) { }

	private setImage(imageData: ImageData) {
		if (this.texture) {
			this.texture.dispose();
		}
		this.texture = new GLTexture(this.gl, imageData);

		const gl = this.gl;
        gl.bindTexture(gl.TEXTURE_2D, this.texture.tex);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
        gl.generateMipmap(gl.TEXTURE_2D);
        gl.bindTexture(gl.TEXTURE_2D, null);

		this.currentImageData = imageData;
		this.isNoCover = false;
        this.baseColor = this.computeDominantColor(imageData);
	}

	setFPS(fps: number) {
		this.maxFPS = fps;
	}

	pause() {
		this.isPaused = true;
	}

	resume() {
		this.isPaused = false;
	}

	constructor(canvas: HTMLCanvasElement, options: AppleBackgroundRenderOptions = {}) {
		super(canvas);
		const gl = canvas.getContext('webgl2');
		if (!gl) throw new Error("WebGL 2.0 not supported");
		this.gl = gl;
        
        if (!gl.getExtension('EXT_color_buffer_float')) {
            throw new Error("This browser does not support rendering to floating point textures (EXT_color_buffer_float), which is required for the performance optimization.");
        }

        this.saturation = options.saturation ?? this.saturation;
        this.blurLevel = options.blurLevel ?? this.blurLevel;
        const initRotation = options.rotationSpeed ?? this.baseFlowSpeed;
        this.baseFlowSpeed = initRotation;
        this.timeMultiplier = initRotation;
        this.flowStrength = options.flowStrength ?? this.flowStrength;
        this.distortionStrength = options.distortionStrength ?? this.distortionStrength;
        this.brightness = options.brightness ?? this.brightness;
        this.baseColor = options.baseColor ?? this.baseColor;

		this.mainProgram = new GLProgram(gl, mainVertexShader, mainFragmentShader, "main");
        this.flowMapProgram = new GLProgram(gl, mainVertexShader, flowMapFragmentShader, "flowmap");
		this.initBuffers();
        this.initFlowMapFramebuffer();
	}

	private initBuffers() {
		const gl = this.gl;
		this.positionBuffer = gl.createBuffer()!;
		gl.bindBuffer(gl.ARRAY_BUFFER, this.positionBuffer);
		gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]), gl.STATIC_DRAW);

		this.texCoordBuffer = gl.createBuffer()!;
		gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer);
		gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]), gl.STATIC_DRAW);

		this.indexBuffer = gl.createBuffer()!;
		gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.indexBuffer);
		gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array([0, 1, 2, 0, 2, 3]), gl.STATIC_DRAW);
	}

    private initFlowMapFramebuffer() {
        const gl = this.gl;
        // Reduce resolution for better performance
        const width = Math.max(1, Math.floor(this.canvas.width * this.renderScale));
        const height = Math.max(1, Math.floor(this.canvas.height * this.renderScale));

        this.flowMapFramebuffer = gl.createFramebuffer()!;
        this.flowMapTexture = new GLTexture(gl, width, height, {
            internalFormat: gl.RGBA16F,
            format: gl.RGBA,
            type: gl.FLOAT
        });
        gl.bindFramebuffer(gl.FRAMEBUFFER, this.flowMapFramebuffer);
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.flowMapTexture.tex, 0);
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    }

    private updateFramebufferSize() {
        if (this.currentSize[0] === this.canvas.width && this.currentSize[1] === this.canvas.height) return;
        
        this.currentSize = vec2.fromValues(this.canvas.width, this.canvas.height);
        // Reduce resolution for better performance
        const width = Math.max(1, Math.floor(this.canvas.width * this.renderScale));
        const height = Math.max(1, Math.floor(this.canvas.height * this.renderScale));
        
        if (this.flowMapTexture) {
            this.flowMapTexture.dispose();
        }
        this.flowMapTexture = new GLTexture(this.gl, width, height, {
            internalFormat: this.gl.RGBA16F,
            format: this.gl.RGBA,
            type: this.gl.FLOAT
        });
        this.gl.bindFramebuffer(this.gl.FRAMEBUFFER, this.flowMapFramebuffer!);
        this.gl.framebufferTexture2D(this.gl.FRAMEBUFFER, this.gl.COLOR_ATTACHMENT0, this.gl.TEXTURE_2D, this.flowMapTexture.tex, 0);
        this.gl.bindFramebuffer(this.gl.FRAMEBUFFER, null);
    }

	private render() {
		if (!this.texture || !this.currentImageData || !this.flowMapTexture) return;

		const gl = this.gl;
        this.updateFramebufferSize();

        // Pass 1: Render Flow Map
        gl.bindFramebuffer(gl.FRAMEBUFFER, this.flowMapFramebuffer!);
        gl.viewport(0, 0, this.flowMapTexture.width, this.flowMapTexture.height);
        gl.clear(gl.COLOR_BUFFER_BIT);

        this.flowMapProgram.use();
        const finalTimeMultiplier = this.hasLyric ? this.timeMultiplier * 0.5 : this.timeMultiplier;
        this.flowMapProgram.setUniform1f("time", this.frameTime * finalTimeMultiplier);
        
        gl.bindBuffer(gl.ARRAY_BUFFER, this.positionBuffer!);
        gl.vertexAttribPointer(this.flowMapProgram.attrs["a_position"], 2, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(this.flowMapProgram.attrs["a_position"]);
        
        gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer!);
        gl.vertexAttribPointer(this.flowMapProgram.attrs["a_texCoord"], 2, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(this.flowMapProgram.attrs["a_texCoord"]);
        
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.indexBuffer!);
        gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);

        // Pass 2: Render Main Scene
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.viewport(0, 0, this.canvas.width, this.canvas.height);
        gl.clear(gl.COLOR_BUFFER_BIT);

        this.mainProgram.use();
        this.mainProgram.setUniform1f("time", this.frameTime * finalTimeMultiplier);
		this.mainProgram.setUniform1f("u_saturation", this.saturation);
		this.mainProgram.setUniform1f("u_blurLevel", this.blurLevel);
        this.mainProgram.setUniform1f("u_flowStrength", this.flowStrength);
        this.mainProgram.setUniform1f("u_distortionStrength", this.distortionStrength);
        this.mainProgram.setUniform1f("u_globalBrightness", this.brightness);
        this.mainProgram.setUniform3f("u_baseColor", this.baseColor[0], this.baseColor[1], this.baseColor[2]);

        gl.activeTexture(gl.TEXTURE0);
		this.texture?.bind();
        this.mainProgram.setUniform1i("u_texture", 0);
        
        gl.activeTexture(gl.TEXTURE1);
        this.flowMapTexture!.bind();
        this.mainProgram.setUniform1i("u_flowMap", 1);
        
        gl.bindBuffer(gl.ARRAY_BUFFER, this.positionBuffer!);
        gl.vertexAttribPointer(this.mainProgram.attrs["a_position"], 2, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(this.mainProgram.attrs["a_position"]);
        
        gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer!);
        gl.vertexAttribPointer(this.mainProgram.attrs["a_texCoord"], 2, gl.FLOAT, false, 0, 0);
        gl.enableVertexAttribArray(this.mainProgram.attrs["a_texCoord"]);
        
        gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.indexBuffer!);
        gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
	}

	private tick = () => {
		if (this._disposed) return;
        if (this.isPaused || this.isStatic) {
            this.tickHandle = requestAnimationFrame(this.tick);
            return;
        }

		const now = performance.now();
		const delta = now - this.lastTickTime;
		this.lastTickTime = now;
        
        const frameInterval = 1000 / this.maxFPS;
        if (delta < frameInterval) {
            this.tickHandle = requestAnimationFrame(this.tick);
            return;
        }

        this.frameTime += delta / 1000;
        this.render();

		this.tickHandle = requestAnimationFrame(this.tick);
	};

	start() {
		if (this.isNoCover) return;
		this.lastTickTime = performance.now();
		this.tick();
	}

	dispose() {
		if (this._disposed) return;
		this._disposed = true;
		
		const gl = this.gl;
		if (this.tickHandle) cancelAnimationFrame(this.tickHandle);
		if (this.texture) this.texture.dispose();
        if (this.flowMapTexture) this.flowMapTexture.dispose();
        if (this.flowMapFramebuffer) gl.deleteFramebuffer(this.flowMapFramebuffer);
		this.mainProgram.dispose();
        this.flowMapProgram.dispose();
		if (this.positionBuffer) gl.deleteBuffer(this.positionBuffer);
		if (this.texCoordBuffer) gl.deleteBuffer(this.texCoordBuffer);
		if (this.indexBuffer) gl.deleteBuffer(this.indexBuffer);
	}

	async setAlbum(albumSource: string | HTMLImageElement | HTMLVideoElement, isVideo?: boolean) {
		let imageData: ImageData;
		
		if (typeof albumSource === "string") {
			const img = new Image();
			img.crossOrigin = "anonymous";
			await new Promise((resolve, reject) => {
				img.onload = resolve;
				img.onerror = reject;
				img.src = albumSource;
			});
			
			const canvas = document.createElement("canvas");
			canvas.width = img.width;
			canvas.height = img.height;
			const ctx = canvas.getContext("2d")!;
			ctx.drawImage(img, 0, 0);
			imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
		} else if (albumSource instanceof HTMLImageElement) {
			const canvas = document.createElement("canvas");
			canvas.width = albumSource.width;
			canvas.height = albumSource.height;
			const ctx = canvas.getContext("2d")!;
			ctx.drawImage(albumSource, 0, 0);
			imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
		} else {
			const canvas = document.createElement("canvas");	
			canvas.width = albumSource.videoWidth;
			canvas.height = albumSource.videoHeight;
			const ctx = canvas.getContext("2d")!;
			ctx.drawImage(albumSource, 0, 0);
			imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
		}
		
		this.setImage(imageData);
	}

	setLowFreqVolume(volume: number) {
		// This method is required by the base class.
        // It can be implemented to allow external control over the animation intensity
        // based on low-frequency audio input, similar to the internal dynamic flow.
	}

	setHasLyric(hasLyric: boolean) {
		this.hasLyric = hasLyric;
	}

	setVolume(volume: number) {
        this.volumeFactor = Math.max(0, volume);
		this.setFlowSpeed(this.baseFlowSpeed);
	}

	setStaticMode(staticMode: boolean) {
		this.isStatic = staticMode;
        if (staticMode) {
            // Render one final frame to show the static state
            this.render();
        }
	}
}
